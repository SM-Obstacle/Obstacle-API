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
    Database, RedisPool, internal,
    opt_event::OptEvent,
    ranks::{self, update_leaderboard},
    redis_key::map_key,
    sync,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait as _, Func, IntoValueTuple, Query},
};

use crate::{
    cursors::{ConnectionParameters, RecordDateCursor, RecordRankCursor},
    error::{self, ApiGqlError, GqlResult},
    loaders::{map::MapLoader, player::PlayerLoader},
    objects::{
        event_edition::EventEdition, player::Player, player_rating::PlayerRating,
        ranked_record::RankedRecord, records_filter::RecordsFilter,
        related_edition::RelatedEdition, sort::MapRecordSort, sort_order::SortOrder,
        sort_state::SortState, sortable_fields::MapRecordSortableField,
    },
    utils::{
        page_input::{PaginationDirection, PaginationInput, apply_cursor_input},
        records_filter::apply_filter,
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
    redis_pool: &RedisPool,
    map_id: u32,
    event: OptEvent<'_>,
    rank_sort_by: Option<SortState>,
    date_sort_by: Option<SortState>,
) -> GqlResult<Vec<RankedRecord>> {
    let key = map_key(map_id, event);

    update_leaderboard(conn, redis_pool, map_id, event).await?;

    let to_reverse = matches!(rank_sort_by, Some(SortState::Reverse));
    let record_ids: Vec<i32> = {
        let mut redis_conn = redis_pool.get().await?;

        if to_reverse {
            redis_conn.zrevrange(&key, 0, 99)
        } else {
            redis_conn.zrange(&key, 0, 99)
        }
        .await?
    };

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

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, map_id, record.time, event).await?;
        ranked_records.push(records::RankedRecord { rank, record }.into());
    }

    Ok(ranked_records)
}

enum MapRecordCursor {
    Date(RecordDateCursor),
    Rank(RecordRankCursor),
}

impl IntoValueTuple for &MapRecordCursor {
    fn into_value_tuple(self) -> sea_orm::sea_query::ValueTuple {
        match self {
            MapRecordCursor::Date(record_date_cursor) => {
                IntoValueTuple::into_value_tuple(record_date_cursor)
            }
            MapRecordCursor::Rank(record_rank_cursor) => {
                IntoValueTuple::into_value_tuple(record_rank_cursor)
            }
        }
    }
}

pub(crate) async fn get_map_records_connection<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    map_id: u32,
    event: OptEvent<'_>,
    connection_parameters: ConnectionParameters,
    sort: Option<MapRecordSort>,
    filter: Option<RecordsFilter>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let pagination_input = match sort.map(|s| s.field) {
        Some(MapRecordSortableField::Date) => {
            PaginationInput::<RecordDateCursor>::try_from_input(connection_parameters)?
                .map_cursor(MapRecordCursor::Date)
        }
        _ => PaginationInput::<RecordRankCursor>::try_from_input(connection_parameters)?
            .map_cursor(MapRecordCursor::Rank),
    };

    let base_query = apply_filter(
        global_records::Entity::find().filter(global_records::Column::MapId.eq(map_id)),
        filter.as_ref(),
    );

    let mut query = match sort.map(|s| s.field) {
        Some(MapRecordSortableField::Date) => base_query.cursor_by((
            global_records::Column::RecordDate,
            global_records::Column::RecordId,
        )),
        _ => base_query.cursor_by((
            global_records::Column::Time,
            global_records::Column::RecordDate,
            global_records::Column::RecordId,
        )),
    };

    apply_cursor_input(&mut query, &pagination_input);

    match sort.and_then(|s| s.order) {
        Some(SortOrder::Descending) => query.desc(),
        _ => query.asc(),
    };

    let (mut connection, records) = match pagination_input.dir {
        PaginationDirection::After { cursor } => {
            let records = query
                .first(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            (
                connection::Connection::new(
                    cursor.is_some(),
                    records.len() > pagination_input.limit,
                ),
                itertools::Either::Left(records.into_iter().take(pagination_input.limit)),
            )
        }
        PaginationDirection::Before { .. } => {
            let records = query
                .last(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            let amount_to_skip = records.len().saturating_sub(pagination_input.limit);
            (
                connection::Connection::new(records.len() > pagination_input.limit, true),
                itertools::Either::Right(records.into_iter().skip(amount_to_skip)),
            )
        }
    };

    connection.edges.reserve(records.len());

    let cursor_encoder = match sort.map(|s| s.field) {
        Some(MapRecordSortableField::Date) => |record: &global_records::Model| {
            RecordDateCursor(record.record_date.and_utc(), record.record_id).encode_cursor()
        },
        _ => |record: &global_records::Model| {
            RecordRankCursor {
                time: record.time,
                record_date: record.record_date.and_utc(),
                data: record.record_id,
            }
            .encode_cursor()
        },
    };

    ranks::update_leaderboard(conn, redis_pool, map_id, event).await?;

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

        connection.edges.push(connection::Edge::new(
            ID(cursor_encoder(&record)),
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
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

        records_lib::assert_future_send(sync::transaction(&db.sql_conn, async |txn| {
            get_map_records(
                txn,
                &db.redis_pool,
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
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = gql_ctx.data_unchecked::<Database>();

        records_lib::assert_future_send(sync::transaction(&db.sql_conn, async |txn| {
            connection::query(
                after,
                before,
                first,
                last,
                |after, before, first, last| async move {
                    get_map_records_connection(
                        txn,
                        &db.redis_pool,
                        self.inner.id,
                        event,
                        ConnectionParameters {
                            after,
                            before,
                            first,
                            last,
                        },
                        sort,
                        filter,
                    )
                    .await
                },
            )
            .await
            .map_err(error::map_gql_err)
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

    async fn score(&self) -> f64 {
        self.inner.score
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
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        self.get_records_connection(
            ctx,
            Default::default(),
            after,
            before,
            first,
            last,
            sort,
            filter,
        )
        .await
    }
}
