use async_graphql::{ID, connection};
use entity::{event as event_entity, event_edition, global_records, players, records};
use records_lib::{
    Database, RedisConnection, must, opt_event::OptEvent, ranks::get_rank, transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, QueryFilter as _, QueryOrder as _,
    QuerySelect as _, StreamTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::{
    error::{ApiGqlError, GqlResult},
    objects::{
        event::Event,
        event_edition::EventEdition,
        map::Map,
        mappack::{self, Mappack},
        player::Player,
        ranked_record::RankedRecord,
        sort::UnorderedRecordSort,
        sort_order::SortOrder,
        sort_state::SortState,
    },
    records_connection::{ConnectionParameters, decode_cursor, encode_cursor},
};

pub struct QueryRoot;

async fn get_record<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    record_id: u32,
    event: OptEvent<'_>,
) -> GqlResult<RankedRecord> {
    let record = records::Entity::find_by_id(record_id).one(conn).await?;

    let Some(record) = record else {
        return Err(ApiGqlError::from_record_not_found_error(record_id));
    };

    let out = records::RankedRecord {
        rank: get_rank(
            conn,
            redis_conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?,
        record,
    }
    .into();

    Ok(out)
}

async fn get_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    date_sort_by: Option<SortState>,
    event: OptEvent<'_>,
) -> GqlResult<Vec<RankedRecord>> {
    let records = global_records::Entity::find()
        .order_by(
            global_records::Column::RecordDate,
            match date_sort_by {
                Some(SortState::Reverse) => sea_orm::Order::Asc,
                _ => sea_orm::Order::Desc,
            },
        )
        .limit(100)
        .all(conn)
        .await?;

    let mut ranked_records = Vec::with_capacity(records.len());

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;

        ranked_records.push(
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        );
    }

    Ok(ranked_records)
}

async fn get_records_connection<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    ConnectionParameters {
        after,
        before,
        first,
        last,
    }: ConnectionParameters,
    sort: Option<UnorderedRecordSort>,
    event: OptEvent<'_>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
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
            let decoded = decode_cursor(&cursor)
                .map_err(|e| ApiGqlError::from_cursor_decode_error("after", cursor.0, e))?;
            Some(decoded)
        }
        None => None,
    };

    let before_timestamp = match before {
        Some(cursor) => {
            let decoded = decode_cursor(&cursor)
                .map_err(|e| ApiGqlError::from_cursor_decode_error("before", cursor.0, e))?;
            Some(decoded)
        }
        None => None,
    };

    // Build query with appropriate ordering
    let mut query = global_records::Entity::find();

    // Apply cursor filters
    if let Some(timestamp) = after_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| ApiGqlError::from_invalid_timestamp("after", timestamp))?
            .naive_utc();

        query = query.filter(global_records::Column::RecordDate.lt(dt));
    }

    if let Some(timestamp) = before_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| ApiGqlError::from_invalid_timestamp("before", timestamp))?
            .naive_utc();

        query = query.filter(global_records::Column::RecordDate.gt(dt));
    }

    // Apply ordering based on date_sort_by and pagination direction
    let order = match (sort.and_then(|s| s.order), is_backward) {
        (Some(SortOrder::Descending), false) => sea_orm::Order::Asc,
        (Some(SortOrder::Descending), true) => sea_orm::Order::Desc,
        (_, false) => sea_orm::Order::Desc, // Default: newest first
        (_, true) => sea_orm::Order::Asc,   // Backward pagination: reverse order
    };

    query = query.order_by(global_records::Column::RecordDate, order);

    // Fetch one extra to determine if there's a next/previous page
    query = query.limit((limit + 1) as u64);

    let mut records = query.all(conn).await?;

    // If backward pagination, reverse the results
    if is_backward {
        records.reverse();
    }

    let mut connection = connection::Connection::new(has_previous_page, records.len() > limit);
    connection.edges.reserve(records.len());

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;

        connection.edges.push(connection::Edge::new(
            ID(encode_cursor(&record.record_date.and_utc())),
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        ));
    }

    Ok(connection)
}

#[async_graphql::Object]
impl QueryRoot {
    async fn event_edition_from_mx_id(
        &self,
        ctx: &async_graphql::Context<'_>,
        mx_id: i64,
    ) -> GqlResult<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_edition::Entity::find()
            .filter(event_edition::Column::MxId.eq(mx_id))
            .one(conn)
            .await?;

        Ok(match edition {
            Some(edition) => Some(EventEdition::from_inner(conn, edition).await?),
            None => None,
        })
    }

    async fn mappack(
        &self,
        ctx: &async_graphql::Context<'_>,
        mappack_id: String,
    ) -> GqlResult<Mappack> {
        let res = mappack::get_mappack(ctx, mappack_id).await?;
        Ok(res)
    }

    async fn event(&self, ctx: &async_graphql::Context<'_>, handle: String) -> GqlResult<Event> {
        let conn = ctx.data_unchecked::<DbConn>();
        let event = must::have_event_handle(conn, &handle).await?;

        Ok(event.into())
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<Event>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let r = event_entity::Entity::find()
            .reverse_join(event_edition::Entity)
            .filter(
                Expr::col(event_edition::Column::StartDate)
                    .lt(Func::cust("SYSDATE"))
                    .and(
                        event_edition::Column::Ttl
                            .is_null()
                            .or(Func::cust("SYSDATE").lt(Func::cust("TIMESTAMPADD")
                                .arg(Expr::custom_keyword("SECOND"))
                                .arg(Expr::col(event_edition::Column::Ttl))
                                .arg(Expr::col(event_edition::Column::StartDate)))),
                    ),
            )
            .group_by(event_entity::Column::Id)
            .into_model()
            .all(conn)
            .await?;

        Ok(r)
    }

    async fn record(
        &self,
        ctx: &async_graphql::Context<'_>,
        record_id: u32,
    ) -> GqlResult<RankedRecord> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_record(txn, &mut redis_conn, record_id, Default::default()).await
        })
        .await
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>, game_id: String) -> GqlResult<Map> {
        let conn = ctx.data_unchecked::<DbConn>();

        let opt_map = records_lib::map::get_map_from_uid(conn, &game_id).await?;

        opt_map
            .ok_or_else(|| ApiGqlError::from_map_not_found_error(game_id))
            .map(From::from)
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>, login: String) -> GqlResult<Player> {
        let conn = ctx.data_unchecked::<DbConn>();

        let opt_player = players::Entity::find()
            .filter(players::Column::Login.eq(&login))
            .one(conn)
            .await?;

        opt_player
            .ok_or_else(|| ApiGqlError::from_player_not_found_error(login))
            .map(From::from)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> GqlResult<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_records(txn, &mut redis_conn, date_sort_by, Default::default()).await
        })
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
        sort: Option<UnorderedRecordSort>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            connection::query(
                after,
                before,
                first,
                last,
                |after, before, first, last| async move {
                    get_records_connection(
                        txn,
                        &mut redis_conn,
                        ConnectionParameters {
                            after,
                            before,
                            first,
                            last,
                        },
                        sort,
                        Default::default(),
                    )
                    .await
                },
            )
            .await
            .map_err(ApiGqlError::from_gql_error)
        })
        .await
    }
}
