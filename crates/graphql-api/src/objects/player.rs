use async_graphql::{Enum, ID, connection};
use entity::{global_records, maps, players, records, role};
use records_lib::{
    RedisConnection, RedisPool, error::RecordsError, internal, opt_event::OptEvent,
    ranks::get_rank, transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, JoinType,
    QueryFilter as _, QueryOrder as _, QuerySelect as _, RelationTrait, StreamTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::{
    objects::{
        ranked_record::RankedRecord, records_filter::RecordsFilter, sort::UnorderedRecordSort,
        sort_order::SortOrder, sort_state::SortState,
    },
    records_connection::{ConnectionParameters, decode_cursor, encode_cursor},
};

#[derive(Copy, Clone, Eq, PartialEq, Enum)]
#[repr(u8)]
enum PlayerRole {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl TryFrom<role::Model> for PlayerRole {
    type Error = RecordsError;

    fn try_from(role: role::Model) -> Result<Self, Self::Error> {
        if role.id < 3 {
            // SAFETY: enum is repr(u8) and role id is in range
            Ok(unsafe { std::mem::transmute::<u8, PlayerRole>(role.id) })
        } else {
            Err(RecordsError::UnknownRole(role.id, role.role_name))
        }
    }
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct Player {
    #[sea_orm(nested)]
    pub inner: players::Model,
}

impl From<players::Model> for Player {
    fn from(inner: players::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Player {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Player:{}", self.inner.id))
    }

    async fn login(&self) -> &str {
        &self.inner.login
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn zone_path(&self) -> Option<&str> {
        self.inner.zone_path.as_deref()
    }

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<PlayerRole> {
        let conn = ctx.data_unchecked::<DbConn>();

        let r = role::Entity::find_by_id(self.inner.role)
            .one(conn)
            .await?
            .ok_or_else(|| internal!("Role with ID {} must exist in database", self.inner.role))?
            .try_into()?;

        Ok(r)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = ctx.data_unchecked::<RedisPool>().get().await?;

        records_lib::assert_future_send(transaction::within(conn, async |txn| {
            get_player_records(
                txn,
                &mut redis_conn,
                self.inner.id,
                Default::default(),
                date_sort_by,
            )
            .await
        }))
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
        #[graphql(desc = "Filter options for records")] filter: Option<RecordsFilter>,
    ) -> async_graphql::Result<connection::Connection<ID, RankedRecord>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = ctx.data_unchecked::<RedisPool>().get().await?;

        records_lib::assert_future_send(transaction::within(conn, async |txn| {
            connection::query(
                after,
                before,
                first,
                last,
                |after, before, first, last| async move {
                    get_player_records_connection(
                        txn,
                        &mut redis_conn,
                        self.inner.id,
                        Default::default(),
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
        }))
        .await
    }
}

async fn get_player_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    player_id: u32,
    event: OptEvent<'_>,
    date_sort_by: Option<SortState>,
) -> async_graphql::Result<Vec<RankedRecord>> {
    // Query the records with these ids

    let records = global_records::Entity::find()
        .filter(global_records::Column::RecordPlayerId.eq(player_id))
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

async fn get_player_records_connection<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    player_id: u32,
    event: OptEvent<'_>,
    ConnectionParameters {
        after,
        before,
        first,
        last,
    }: ConnectionParameters,
    sort: Option<UnorderedRecordSort>,
    filter: Option<RecordsFilter>,
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
    let mut query =
        global_records::Entity::find().filter(global_records::Column::RecordPlayerId.eq(player_id));

    // Apply filters if provided
    if let Some(filter) = filter {
        // Join with maps table if needed for map filters
        if filter.map_uid.is_some() || filter.map_name.is_some() {
            query = query.join_as(
                JoinType::InnerJoin,
                global_records::Relation::Maps.def(),
                "m",
            );
        }

        // Apply map UID filter
        if let Some(uid) = filter.map_uid {
            query = query.filter(Expr::col(("m", maps::Column::GameId)).like(format!("%{uid}%")));
        }

        // Apply map name filter
        if let Some(name) = filter.map_name {
            query = query.filter(
                Func::cust("rm_mp_style")
                    .arg(Expr::col(("m", maps::Column::Name)))
                    .like(format!("%{name}%")),
            );
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
