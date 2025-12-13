use async_graphql::connection::CursorType as _;
use async_graphql::{Enum, ID, connection};
use entity::{global_records, maps, players, records, role};
use records_lib::{Database, ranks};
use records_lib::{RedisPool, error::RecordsError, internal, opt_event::OptEvent, sync};
use sea_orm::Order;
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, JoinType,
    QueryFilter as _, QueryOrder as _, QuerySelect as _, RelationTrait, StreamTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::objects::records_filter::RecordsFilter;
use crate::{
    cursors::ConnectionParameters,
    error::{ApiGqlError, GqlResult},
    objects::{ranked_record::RankedRecord, sort_state::SortState},
};

use crate::{
    cursors::{CURSOR_DEFAULT_LIMIT, CURSOR_LIMIT_RANGE, RecordDateCursor},
    error,
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

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<PlayerRole> {
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
    ) -> GqlResult<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();

        records_lib::assert_future_send(sync::transaction(&db.sql_conn, async |txn| {
            get_player_records(
                txn,
                &db.redis_pool,
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
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();

        records_lib::assert_future_send(sync::transaction(&db.sql_conn, async |txn| {
            connection::query(
                after,
                before,
                first,
                last,
                |after, before, first, last| async move {
                    get_player_records_connection(
                        txn,
                        &db.redis_pool,
                        self.inner.id,
                        Default::default(),
                        ConnectionParameters {
                            after,
                            before,
                            first,
                            last,
                        },
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

async fn get_player_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    player_id: u32,
    event: OptEvent<'_>,
    date_sort_by: Option<SortState>,
) -> GqlResult<Vec<RankedRecord>> {
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

    let mut ranking_session = ranks::RankingSession::try_from_pool(redis_pool).await?;

    for record in records {
        let rank = ranks::get_rank_in_session(
            &mut ranking_session,
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
    redis_pool: &RedisPool,
    player_id: u32,
    event: OptEvent<'_>,
    ConnectionParameters {
        after,
        before,
        first,
        last,
    }: ConnectionParameters,
    filter: Option<RecordsFilter>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
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

    let has_previous_page = after.is_some();

    // Decode cursors if provided
    let after_timestamp = match after {
        Some(cursor) => {
            let decoded = RecordDateCursor::decode_cursor(&cursor)
                .map_err(|e| ApiGqlError::from_cursor_decode_error("after", cursor.0, e))?;
            Some(decoded)
        }
        None => None,
    };

    let before_timestamp = match before {
        Some(cursor) => {
            let decoded = RecordDateCursor::decode_cursor(&cursor)
                .map_err(|e| ApiGqlError::from_cursor_decode_error("before", cursor.0, e))?;
            Some(decoded)
        }
        None => None,
    };

    // Build query with appropriate ordering
    let mut query =
        global_records::Entity::find().filter(global_records::Column::RecordPlayerId.eq(player_id));

    // Apply filters if provided
    if let Some(filter) = filter {
        // Join with maps table if needed for map filters
        if let Some(filter) = filter.map {
            query = query.join_as(
                JoinType::InnerJoin,
                global_records::Relation::Maps.def(),
                "m",
            );

            // Apply map UID filter
            if let Some(uid) = filter.map_uid {
                query =
                    query.filter(Expr::col(("m", maps::Column::GameId)).like(format!("%{uid}%")));
            }

            // Apply map name filter
            if let Some(name) = filter.map_name {
                query = query.filter(
                    Func::cust("rm_mp_style")
                        .arg(Expr::col(("m", maps::Column::Name)))
                        .like(format!("%{name}%")),
                );
            }
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
        query = query.filter(global_records::Column::RecordDate.lt(timestamp.0));
    }

    if let Some(timestamp) = before_timestamp {
        query = query.filter(global_records::Column::RecordDate.gt(timestamp.0));
    }

    // Apply ordering
    query = query.order_by(
        global_records::Column::RecordDate,
        if last.is_some() {
            Order::Asc
        } else {
            Order::Desc
        },
    );

    // Fetch one extra to determine if there's a next page
    query = query.limit((limit + 1) as u64);

    let records = query.all(conn).await?;

    let mut connection = connection::Connection::new(has_previous_page, records.len() > limit);

    let mut ranking_session = ranks::RankingSession::try_from_pool(redis_pool).await?;

    for record in records.into_iter().take(limit) {
        let rank = ranks::get_rank_in_session(
            &mut ranking_session,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;

        connection.edges.push(connection::Edge::new(
            ID(RecordDateCursor(record.record_date.and_utc()).encode_cursor()),
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        ));
    }

    Ok(connection)
}
