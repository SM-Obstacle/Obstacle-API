use async_graphql::{ID, connection};
use entity::{event as event_entity, event_edition, global_records, maps, players, records};
use records_lib::{
    Database, RedisConnection, must, opt_event::OptEvent, ranks::get_rank, transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, QueryFilter as _, QueryOrder as _,
    QuerySelect as _, StreamTrait, JoinType, RelationTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::{
    objects::{
        event::Event,
        event_edition::EventEdition,
        map::Map,
        mappack::{self, Mappack},
        player::Player,
        ranked_record::RankedRecord,
        records_filter::RecordsFilter,
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
) -> async_graphql::Result<RankedRecord> {
    let record = records::Entity::find_by_id(record_id).one(conn).await?;

    let Some(record) = record else {
        return Err(async_graphql::Error::new("Record not found."));
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
) -> async_graphql::Result<Vec<RankedRecord>> {
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
    date_sort_by: Option<SortState>,
    filter: Option<RecordsFilter>,
    event: OptEvent<'_>,
) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
    let limit = if let Some(first) = first {
        if !(1..=100).contains(&first) {
            return Err(async_graphql::Error::new(
                "'first' must be between 1 and 100",
            ));
        }
        first
    } else if let Some(last) = last {
        if !(1..=100).contains(&last) {
            return Err(async_graphql::Error::new(
                "'last' must be between 1 and 100",
            ));
        }
        last
    } else {
        50 // Default limit
    };

    // Decode cursors if provided
    let after_timestamp = if let Some(cursor) = after.as_ref() {
        Some(decode_cursor(cursor).map_err(async_graphql::Error::new)?)
    } else {
        None
    };

    let before_timestamp = if let Some(cursor) = before.as_ref() {
        Some(decode_cursor(cursor).map_err(async_graphql::Error::new)?)
    } else {
        None
    };

    // Determine if we're going forward or backward
    let is_backward = last.is_some() || before.is_some();
    let has_previous_page = after.is_some();

    // Build query with appropriate ordering
    let mut query = global_records::Entity::find();

    // Apply filters if provided
    if let Some(filter) = filter {
        // Join with players table if needed for player filters
        if filter.player_login.is_some() || filter.player_name.is_some() {
            query = query
                .join(JoinType::InnerJoin, global_records::Relation::Players.def());
        }

        // Join with maps table if needed for map filters
        if filter.map_uid.is_some() || filter.map_name.is_some() {
            query = query
                .join(JoinType::InnerJoin, global_records::Relation::Maps.def());
        }

        // Apply player login filter
        if let Some(login) = filter.player_login {
            query = query.filter(players::Column::Login.eq(login));
        }

        // Apply player name filter
        if let Some(name) = filter.player_name {
            query = query.filter(players::Column::Name.eq(name));
        }

        // Apply map UID filter
        if let Some(uid) = filter.map_uid {
            query = query.filter(maps::Column::GameId.eq(uid));
        }

        // Apply map name filter
        if let Some(name) = filter.map_name {
            query = query.filter(maps::Column::Name.eq(name));
        }

        // Apply date filters
        if let Some(before_date) = filter.before_date {
            query = query.filter(global_records::Column::RecordDate.lt(before_date));
        }

        if let Some(after_date) = filter.after_date {
            query = query.filter(global_records::Column::RecordDate.gt(after_date));
        }

        // Apply time filters
        if let Some(time_gt) = filter.time_gt {
            query = query.filter(global_records::Column::Time.gt(time_gt));
        }

        if let Some(time_lt) = filter.time_lt {
            query = query.filter(global_records::Column::Time.lt(time_lt));
        }

        if let Some(time_eq) = filter.time_eq {
            query = query.filter(global_records::Column::Time.eq(time_eq));
        }
    }

    // Apply cursor filters
    if let Some(timestamp) = after_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| async_graphql::Error::new("Invalid timestamp in cursor"))?
            .naive_utc();

        query = query.filter(global_records::Column::RecordDate.lt(dt));
    }

    if let Some(timestamp) = before_timestamp {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| async_graphql::Error::new("Invalid timestamp in cursor"))?
            .naive_utc();

        query = query.filter(global_records::Column::RecordDate.gt(dt));
    }

    // Apply ordering based on date_sort_by and pagination direction
    let order = match (date_sort_by, is_backward) {
        (Some(SortState::Reverse), false) => sea_orm::Order::Asc,
        (Some(SortState::Reverse), true) => sea_orm::Order::Desc,
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
    ) -> async_graphql::Result<Option<EventEdition<'_>>> {
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
    ) -> async_graphql::Result<Mappack> {
        let res = mappack::get_mappack(ctx, mappack_id).await?;
        Ok(res)
    }

    async fn event(
        &self,
        ctx: &async_graphql::Context<'_>,
        handle: String,
    ) -> async_graphql::Result<Event> {
        let conn = ctx.data_unchecked::<DbConn>();
        let event = must::have_event_handle(conn, &handle).await?;

        Ok(event.into())
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Event>> {
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
    ) -> async_graphql::Result<RankedRecord> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_record(txn, &mut redis_conn, record_id, Default::default()).await
        })
        .await
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
        game_id: String,
    ) -> async_graphql::Result<Map> {
        let conn = ctx.data_unchecked::<DbConn>();

        records_lib::map::get_map_from_uid(conn, &game_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
            .map(Into::into)
    }

    async fn player(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<Player> {
        let conn = ctx.data_unchecked::<DbConn>();

        let player = players::Entity::find()
            .filter(players::Column::Login.eq(login))
            .one(conn)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))?;

        Ok(player.into())
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_records(txn, &mut redis_conn, date_sort_by, Default::default()).await
        })
        .await
    }

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
        date_sort_by: Option<SortState>,
        #[graphql(desc = "Filter options for records")] filter: Option<RecordsFilter>,
    ) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
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
                        date_sort_by,
                        filter,
                        Default::default(),
                    )
                    .await
                },
            )
            .await
        })
        .await
    }
}
