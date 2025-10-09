use async_graphql::{ID, connection, dataloader::DataLoader};
use deadpool_redis::redis::AsyncCommands as _;
use entity::{
    event_edition, event_edition_maps, global_event_records, global_records, maps, player_rating,
    players, records,
};
use records_lib::{
    Database, RedisConnection, internal,
    opt_event::OptEvent,
    ranks::{get_rank, update_leaderboard},
    redis_key::map_key,
    transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, JoinType,
    QueryFilter as _, QueryOrder as _, QuerySelect as _, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait as _, Func, Query},
};

use crate::{
    error::{ApiGqlError, GqlResult},
    loaders::{map::MapLoader, player::PlayerLoader},
    objects::{
        event_edition::EventEdition, player::Player, player_rating::PlayerRating,
        ranked_record::RankedRecord, records_filter::RecordsFilter,
        related_edition::RelatedEdition, sort::MapRecordSort, sort_order::SortOrder,
        sort_state::SortState, sortable_fields::MapRecordSortableField,
    },
    records_connection::{ConnectionParameters, decode_cursor, encode_cursor},
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
    filter: Option<RecordsFilter>,
) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
    let limit = if let Some(first) = first {
        if !(1..=100).contains(&first) {
            return Err(ApiGqlError::from_cursor_range_error(
                "first",
                1..=100,
                first,
            ));
        }
        first
    } else if let Some(last) = last {
        if !(1..=100).contains(&last) {
            return Err(ApiGqlError::from_cursor_range_error("last", 1..=100, last));
        }
        last
    } else {
        50 // Default limit
    };

    // Determine if we're going forward or backward
    let is_backward = last.is_some() || before.is_some();
    let has_previous_page = after.is_some();

    // Decode cursors if provided
    let after_timestamp = match after {
        Some(cursor) => {
            let decoded = decode_cursor(&cursor).map_err(|decode_err| {
                ApiGqlError::from_cursor_decode_error("after", cursor.0, decode_err)
            })?;
            Some(decoded)
        }
        None => None,
    };

    let before_timestamp = match before {
        Some(cursor) => {
            let decoded = decode_cursor(&cursor).map_err(|decode_err| {
                ApiGqlError::from_cursor_decode_error("before", cursor.0, decode_err)
            })?;
            Some(decoded)
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

    // Apply filters if provided
    if let Some(filter) = &filter {
        // For player filters, we need to join with players table
        if filter.player_login.is_some() || filter.player_name.is_some() {
            select.join_as(
                JoinType::InnerJoin,
                players::Entity,
                "p",
                Expr::col(("r", records::Column::RecordPlayerId))
                    .equals(("p", players::Column::Id)),
            );

            if let Some(ref login) = filter.player_login {
                select
                    .and_where(Expr::col(("p", players::Column::Login)).like(format!("%{login}%")));
            }

            if let Some(ref name) = filter.player_name {
                select.and_where(
                    Func::cust("rm_mp_style")
                        .arg(Expr::col(("p", players::Column::Name)))
                        .like(format!("%{name}%")),
                );
            }
        }

        // Apply date filters
        if let Some(before_date) = filter.before_date {
            select.and_where(Expr::col(("r", records::Column::RecordDate)).lt(before_date));
        }

        if let Some(after_date) = filter.after_date {
            select.and_where(Expr::col(("r", records::Column::RecordDate)).gt(after_date));
        }

        // Apply time filters
        if let Some(time_gt) = filter.time_gt {
            select.and_where(Expr::col(("r", records::Column::Time)).gt(time_gt));
        }

        if let Some(time_lt) = filter.time_lt {
            select.and_where(Expr::col(("r", records::Column::Time)).lt(time_lt));
        }
    }

    // Apply cursor filters
    if let Some(timestamp) = after_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| ApiGqlError::from_invalid_timestamp("after", timestamp))?
            .naive_utc();

        select.and_where(Expr::col(("r", records::Column::RecordDate)).lt(dt));
    }

    if let Some(timestamp) = before_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| ApiGqlError::from_invalid_timestamp("before", timestamp))?
            .naive_utc();

        select.and_where(Expr::col(("r", records::Column::RecordDate)).gt(dt));
    }

    // Apply ordering based on date_sort_by and pagination direction
    if let Some(sort) = sort {
        match sort.field {
            MapRecordSortableField::Date => {
                let order = match (sort.order, is_backward) {
                    (Some(SortOrder::Descending), false) => sea_orm::Order::Asc,
                    (Some(SortOrder::Descending), true) => sea_orm::Order::Desc,
                    (_, false) => sea_orm::Order::Desc,
                    (_, true) => sea_orm::Order::Asc,
                };

                select.order_by_expr(Expr::col(("r", records::Column::RecordDate)).into(), order);
            }
            MapRecordSortableField::Rank => {
                // For rank-based sorting with pagination, we need to fetch player IDs from Redis
                // This is complex and may not work well with cursors
                // For now, we'll order by time which correlates with rank

                let order = match (sort.order, is_backward) {
                    (Some(SortOrder::Descending), false) => sea_orm::Order::Desc,
                    (Some(SortOrder::Ascending), true) => sea_orm::Order::Asc,
                    (_, false) => sea_orm::Order::Asc,
                    (_, true) => sea_orm::Order::Desc,
                };

                select.order_by_expr(Expr::col(("r", records::Column::Time)).into(), order);
                select.order_by_expr(
                    Expr::col(("r", records::Column::RecordDate)).into(),
                    sea_orm::Order::Asc,
                );
            }
        }
    } else {
        // Default ordering by record date
        let order = if is_backward {
            sea_orm::Order::Asc
        } else {
            sea_orm::Order::Desc
        };
        select.order_by_expr(Expr::col(("r", records::Column::RecordDate)).into(), order);
    }

    // Fetch one extra to determine if there's a next/previous page
    select.limit((limit + 1) as u64);

    let stmt = conn.get_database_backend().build(&*select);
    let mut records = conn
        .query_all(stmt)
        .await?
        .into_iter()
        .map(|result| records::Model::from_query_result(&result, ""))
        .collect::<Result<Vec<_>, _>>()?;

    // If backward pagination, reverse the results
    if is_backward {
        records.reverse();
    }

    let mut connection = connection::Connection::new(has_previous_page, records.len() > limit);

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
            ID(encode_cursor(&record.record_date.and_utc())),
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
        filter: Option<RecordsFilter>,
    ) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
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
                        filter,
                    )
                    .await
                },
            )
            .await
            .map_err(ApiGqlError::from_gql_error)
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
        #[graphql(desc = "Filter options for records")] filter: Option<RecordsFilter>,
    ) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
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
