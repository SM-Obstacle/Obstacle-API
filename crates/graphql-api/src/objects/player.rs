use async_graphql::{
    Enum, ID,
    connection::{self, CursorType as _},
};
use entity::{global_records, players, records, role};
use records_lib::{
    RedisConnection, RedisPool, error::RecordsError, internal, opt_event::OptEvent,
    ranks::get_rank, transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, StreamTrait,
};

use crate::{
    error::{self, ApiGqlError, GqlResult},
    objects::{
        ranked_record::RankedRecord, sort::UnorderedRecordSort, sort_order::SortOrder,
        sort_state::SortState,
    },
    records_connection::{
        CURSOR_DEFAULT_LIMIT, CURSOR_LIMIT_RANGE, ConnectionParameters, RecordDateCursor,
    },
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
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
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
    redis_conn: &mut RedisConnection,
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
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let sort_order = sort.and_then(|s| s.order).unwrap_or(SortOrder::Descending);

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

    let (before, after) = match sort_order {
        SortOrder::Ascending => (after, before),
        SortOrder::Descending => (before, after),
    };

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

    // Apply cursor filters
    if let Some(timestamp) = after_timestamp {
        query = query.filter(global_records::Column::RecordDate.gt(timestamp.0));
    }

    if let Some(timestamp) = before_timestamp {
        query = query.filter(global_records::Column::RecordDate.lt(timestamp.0));
    }

    // Apply ordering
    query = query.order_by(global_records::Column::RecordDate, sort_order.into());

    // Fetch one extra to determine if there's a next page
    query = query.limit((limit + 1) as u64);

    let records = query.all(conn).await?;

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
