use async_graphql::{Enum, ID, connection};
use entity::{global_records, players, records, role};
use records_lib::{Database, ranks};
use records_lib::{RedisPool, error::RecordsError, internal, opt_event::OptEvent, sync};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, FromQueryResult, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, StreamTrait, TransactionTrait,
};

use crate::cursors::RecordDateCursor;
use crate::objects::records_filter::RecordsFilter;
use crate::objects::root::get_records_connection_impl;
use crate::objects::sort::UnorderedRecordSort;
use crate::utils::records_filter::apply_filter;
use crate::{
    cursors::ConnectionParameters,
    error::GqlResult,
    objects::{ranked_record::RankedRecord, sort_state::SortState},
};

use crate::error;

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

    async fn score(&self) -> f64 {
        self.inner.score
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
        sort: Option<UnorderedRecordSort>,
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();

        connection::query_with(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                get_player_records_connection(
                    &db.sql_conn,
                    &db.redis_pool,
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
        .map_err(error::map_gql_err)
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

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

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

pub(crate) async fn get_player_records_connection<C>(
    conn: &C,
    redis_pool: &RedisPool,
    player_id: u32,
    event: OptEvent<'_>,
    connection_parameters: ConnectionParameters<RecordDateCursor>,
    sort: Option<UnorderedRecordSort>,
    filter: Option<RecordsFilter>,
) -> GqlResult<connection::Connection<ID, RankedRecord>>
where
    C: ConnectionTrait + TransactionTrait,
{
    let base_query = apply_filter(
        global_records::Entity::find().filter(global_records::Column::RecordPlayerId.eq(player_id)),
        filter.as_ref(),
    );

    get_records_connection_impl(
        conn,
        redis_pool,
        connection_parameters,
        event,
        sort,
        base_query,
    )
    .await
}
