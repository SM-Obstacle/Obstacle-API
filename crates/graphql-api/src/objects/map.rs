use async_graphql::{
    ID,
    connection::{self, CursorType},
    dataloader::DataLoader,
};
use deadpool_redis::redis::AsyncCommands as _;
use entity::{
    event_edition, event_edition_maps, global_event_records, global_records, maps, player_rating,
    records,
};
use records_lib::{
    Database, RedisConnection, internal,
    opt_event::OptEvent,
    ranks::{get_rank, update_leaderboard},
    redis_key::map_key,
    transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait as _, Func, Query},
};

use crate::{
    error::{ApiGqlError, GqlResult},
    loaders::{map::MapLoader, player::PlayerLoader},
    objects::{
        event_edition::EventEdition, player::Player, player_rating::PlayerRating,
        ranked_record::RankedRecord, related_edition::RelatedEdition, sort::MapRecordSort,
        sort_order::SortOrder, sort_state::SortState, sortable_fields::MapRecordSortableField,
    },
    records_connection::{
        CURSOR_DEFAULT_LIMIT, CURSOR_LIMIT_RANGE, ConnectionParameters, RecordDateCursor,
        RecordRankCursor,
    },
};

#[derive(FromQueryResult)]
struct RawRelatedEdition {
    map_id: u32,
    #[sea_orm(nested)]
    edition: event_edition::Model,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct Map {
    #[sea_orm(nested)]
    pub inner: maps::Model,
}

impl From<maps::Model> for Map {
    fn from(inner: maps::Model) -> Self {
        Self { inner }
    }
}

async fn get_map_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
    rank_sort_by: Option<SortState>,
    date_sort_by: Option<SortState>,
) -> GqlResult<Vec<RankedRecord>> {
    let key = map_key(map_id, event);

    update_leaderboard(conn, redis_conn, map_id, event).await?;

    let to_reverse = matches!(rank_sort_by, Some(SortState::Reverse));
    let record_ids: Vec<i32> = if to_reverse {
        redis_conn.zrevrange(&key, 0, 99)
    } else {
        redis_conn.zrange(&key, 0, 99)
    }
    .await?;

    if record_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut select = Query::select();

    let select = match event.get() {
        Some((ev, ed)) => select.from_as(global_event_records::Entity, "r").and_where(
            Expr::col(("r", global_event_records::Column::EventId))
                .eq(ev.id)
                .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
        ),
        None => select.from_as(global_records::Entity, "r"),
    }
    .column(Asterisk)
    .and_where(Expr::col(("r", records::Column::MapId)).eq(map_id));

    if let Some(ref s) = date_sort_by {
        select.order_by_expr(
            Expr::col(("r", records::Column::RecordDate)).into(),
            match s {
                SortState::Sort => sea_orm::Order::Desc,
                SortState::Reverse => sea_orm::Order::Asc,
            },
        );
    } else {
        select
            .and_where(Expr::col(("r", records::Column::RecordPlayerId)).is_in(record_ids))
            .order_by_expr(
                Expr::col(("r", records::Column::Time)).into(),
                if to_reverse {
                    sea_orm::Order::Desc
                } else {
                    sea_orm::Order::Asc
                },
            )
            .order_by_expr(
                Expr::col(("r", records::Column::RecordDate)).into(),
                sea_orm::Order::Asc,
            );
    }

    if date_sort_by.is_some() {
        select.limit(100);
    }

    let stmt = conn.get_database_backend().build(&*select);
    let records = conn
        .query_all(stmt)
        .await?
        .into_iter()
        .map(|result| records::Model::from_query_result(&result, ""))
        .collect::<Result<Vec<_>, _>>()?;

    let mut ranked_records = Vec::with_capacity(records.len());

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;
        ranked_records.push(records::RankedRecord { rank, record }.into());
    }

    Ok(ranked_records)
}

enum MapRecordCursor {
    Date(RecordDateCursor),
    Rank(RecordRankCursor),
}

fn encode_map_cursor(cursor: &MapRecordCursor) -> String {
    match cursor {
        MapRecordCursor::Date(date_cursor) => date_cursor.encode_cursor(),
        MapRecordCursor::Rank(rank_cursor) => rank_cursor.encode_cursor(),
    }
}

async fn get_map_records_connection<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
    ConnectionParameters {
        before,
        after,
        first,
        last,
    }: ConnectionParameters,
    sort: Option<MapRecordSort>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let (sort_field, sort_order) = match sort {
        Some(MapRecordSort { field, order }) => (field, order.unwrap_or(SortOrder::Ascending)),
        None => (MapRecordSortableField::Rank, SortOrder::Ascending),
    };

    let limit = if let Some(first) = first {
        if !CURSOR_LIMIT_RANGE.contains(&first) {
            return Err(ApiGqlError::from_cursor_range_error(
                "first",
                CURSOR_LIMIT_RANGE,
                first,
            ));
        }
        first
    } else if let Some(last) = last {
        if !CURSOR_LIMIT_RANGE.contains(&last) {
            return Err(ApiGqlError::from_cursor_range_error(
                "last",
                CURSOR_LIMIT_RANGE,
                last,
            ));
        }
        last
    } else {
        CURSOR_DEFAULT_LIMIT
    };

    let has_next_page = after.is_some();

    // Reverse the cursors when sorting backward
    let (before, after) = match sort_order {
        SortOrder::Descending => (after, before),
        SortOrder::Ascending => (before, after),
    };

    // Decode cursors if provided
    let after = match after {
        Some(cursor) => {
            let cursor = match sort_field {
                MapRecordSortableField::Date => {
                    CursorType::decode_cursor(&cursor).map(MapRecordCursor::Date)
                }
                MapRecordSortableField::Rank => {
                    CursorType::decode_cursor(&cursor).map(MapRecordCursor::Rank)
                }
            }
            .map_err(|e| ApiGqlError::from_cursor_decode_error("after", cursor.0, e))?;

            Some(cursor)
        }
        None => None,
    };

    let before = match before {
        Some(cursor) => {
            let cursor = match sort_field {
                MapRecordSortableField::Date => {
                    CursorType::decode_cursor(&cursor).map(MapRecordCursor::Date)
                }
                MapRecordSortableField::Rank => {
                    CursorType::decode_cursor(&cursor).map(MapRecordCursor::Rank)
                }
            }
            .map_err(|e| ApiGqlError::from_cursor_decode_error("before", cursor.0, e))?;

            Some(cursor)
        }
        None => None,
    };

    let _key = map_key(map_id, event);
    update_leaderboard(conn, redis_conn, map_id, event).await?;

    let mut select = Query::select();

    let select = match event.get() {
        Some((ev, ed)) => select.from_as(global_event_records::Entity, "r").and_where(
            Expr::col(("r", global_event_records::Column::EventId))
                .eq(ev.id)
                .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
        ),
        None => select.from_as(global_records::Entity, "r"),
    }
    .column(Asterisk)
    .and_where(Expr::col(("r", records::Column::MapId)).eq(map_id));

    // Apply cursor filters
    if let Some(cursor) = after {
        let (date, time) = match cursor {
            MapRecordCursor::Date(date) => (date.0, None),
            MapRecordCursor::Rank(rank) => (rank.record_date, Some(rank.time)),
        };

        select.and_where(Expr::col(("r", records::Column::RecordDate)).gt(date));

        if let Some(time) = time {
            select.and_where(Expr::col(("r", records::Column::Time)).gte(time));
        }
    }

    if let Some(cursor) = before {
        let (date, time) = match cursor {
            MapRecordCursor::Date(date) => (date.0, None),
            MapRecordCursor::Rank(rank) => (rank.record_date, Some(rank.time)),
        };

        select.and_where(Expr::col(("r", records::Column::RecordDate)).lt(date));

        if let Some(time) = time {
            select.and_where(Expr::col(("r", records::Column::Time)).lt(time));
        }
    }

    // Apply ordering
    if let MapRecordSortableField::Rank = sort_field {
        select.order_by_expr(
            Expr::col(("r", records::Column::Time)).into(),
            sort_order.into(),
        );
    }
    select.order_by_expr(
        Expr::col(("r", records::Column::RecordDate)).into(),
        sort_order.into(),
    );

    // Fetch one extra to determine if there's a next page
    select.limit((limit + 1) as u64);

    let stmt = conn.get_database_backend().build(&*select);
    let records = conn
        .query_all(stmt)
        .await?
        .into_iter()
        .map(|result| records::Model::from_query_result(&result, ""))
        .collect::<Result<Vec<_>, _>>()?;

    let mut connection = connection::Connection::new(has_next_page, records.len() > limit);

    let encode_cursor_fn = match sort_field {
        MapRecordSortableField::Date => |record: &records::Model| {
            encode_map_cursor(&MapRecordCursor::Date(RecordDateCursor(
                record.record_date.and_utc(),
            )))
        },
        MapRecordSortableField::Rank => |record: &records::Model| {
            encode_map_cursor(&MapRecordCursor::Rank(RecordRankCursor {
                record_date: record.record_date.and_utc(),
                time: record.time,
            }))
        },
    };

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;

        connection.edges.push(connection::Edge::new(
            ID(encode_cursor_fn(&record)),
            records::RankedRecord { rank, record }.into(),
        ));
    }

    Ok(connection)
}

impl Map {
    pub(super) async fn get_records(
        &self,
        gql_ctx: &async_graphql::Context<'_>,
        event: OptEvent<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> GqlResult<Vec<RankedRecord>> {
        let db = gql_ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        records_lib::assert_future_send(transaction::within(&db.sql_conn, async |txn| {
            get_map_records(
                txn,
                &mut redis_conn,
                self.inner.id,
                event,
                rank_sort_by,
                date_sort_by,
            )
            .await
        }))
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn get_records_connection(
        &self,
        gql_ctx: &async_graphql::Context<'_>,
        event: OptEvent<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
        sort: Option<MapRecordSort>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = gql_ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        records_lib::assert_future_send(transaction::within(&db.sql_conn, async |txn| {
            connection::query(
                after,
                before,
                first,
                last,
                |after, before, first, last| async move {
                    get_map_records_connection(
                        txn,
                        &mut redis_conn,
                        self.inner.id,
                        event,
                        ConnectionParameters {
                            after,
                            before,
                            first,
                            last,
                        },
                        sort,
                    )
                    .await
                },
            )
            .await
            .map_err(|e| {
                match e
                    .source
                    .as_ref()
                    .and_then(|source| source.downcast_ref::<ApiGqlError>())
                {
                    Some(err) => err.clone(),
                    None => ApiGqlError::from_gql_error(e),
                }
            })
        }))
        .await
    }
}

#[async_graphql::Object]
impl Map {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Map:{}", self.inner.id))
    }

    async fn game_id(&self) -> &str {
        &self.inner.game_id
    }

    async fn player_id(&self) -> ID {
        ID(format!("v0:Player:{}", self.inner.player_id))
    }

    async fn cps_number(&self) -> Option<u32> {
        self.inner.cps_number
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.player_id)
            .await?
            .ok_or_else(|| {
                ApiGqlError::from(internal!(
                    "author of map {} couldn't be found: {}",
                    self.inner.id,
                    self.inner.player_id
                ))
            })
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn related_event_editions(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<Vec<RelatedEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let raw_editions = event_edition::Entity::find()
            .reverse_join(event_edition_maps::Entity)
            .filter(
                event_edition_maps::Column::MapId
                    .eq(self.inner.id)
                    .or(event_edition_maps::Column::OriginalMapId.eq(self.inner.id)),
            )
            .order_by_desc(event_edition::Column::StartDate)
            .column(event_edition_maps::Column::MapId)
            .into_model::<RawRelatedEdition>()
            .all(conn)
            .await?;

        let mut out = Vec::with_capacity(raw_editions.len());

        let mut maps = map_loader
            .load_many(raw_editions.iter().map(|e| e.map_id))
            .await?;

        for edition in raw_editions {
            let map = maps
                .remove(&edition.map_id)
                .ok_or_else(|| internal!("unknown map id: {}", edition.map_id))?;
            out.push(RelatedEdition {
                map,
                // We want to redirect to the event map page if the edition saves any records
                // on its maps, doesn't have any original map like campaign, or if the map
                // isn't the original one.
                // TODO: this shouldn't be decided by the API actually
                redirect_to_event: edition.edition.is_transparent == 0
                    && edition.edition.save_non_event_record != 0
                    && (edition.edition.non_original_maps != 0 || self.inner.id == edition.map_id),
                edition: EventEdition::from_inner(conn, edition.edition).await?,
            });
        }

        Ok(out)
    }

    async fn average_rating(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<Vec<PlayerRating>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let all = player_rating::Entity::find()
            .filter(player_rating::Column::MapId.eq(self.inner.id))
            .group_by(player_rating::Column::Kind)
            .order_by_asc(player_rating::Column::Kind)
            .select_only()
            .expr_as(1.cast_as("UNSIGNED"), "player_id")
            .columns([player_rating::Column::MapId, player_rating::Column::Kind])
            .expr_as(
                Func::avg(Expr::col(player_rating::Column::Rating)),
                "rating",
            )
            .into_model()
            .all(conn)
            .await?;
        Ok(all)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> GqlResult<Vec<RankedRecord>> {
        self.get_records(ctx, Default::default(), rank_sort_by, date_sort_by)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn records_connection(
        &self,
        ctx: &async_graphql::Context<'_>,
        #[graphql(desc = "Cursor to fetch records after (for forward pagination)")] after: Option<
            String,
        >,
        #[graphql(desc = "Cursor to fetch records before (for backward pagination)")]
        before: Option<String>,
        #[graphql(desc = "Number of records to fetch (default: 50, max: 100)")] first: Option<i32>,
        #[graphql(desc = "Number of records to fetch from the end (for backward pagination)")] last: Option<i32>,
        sort: Option<MapRecordSort>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        self.get_records_connection(ctx, Default::default(), after, before, first, last, sort)
            .await
    }
}
