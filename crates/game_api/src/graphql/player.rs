use std::{collections::HashMap, sync::Arc};

use async_graphql::{Enum, ID, dataloader::Loader};
use entity::{global_records, players, records};
use records_lib::{Database, RedisConnection, models::Role, opt_event::OptEvent, transaction};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult,
    QueryFilter, QueryOrder, QuerySelect, StreamTrait,
};
use sqlx::MySqlPool;

use crate::RecordsErrorKind;

use super::{SortState, ban::Banishment, get_rank, record::RankedRecord};

#[derive(Copy, Clone, Eq, PartialEq, Enum)]
#[repr(u8)]
enum PlayerRole {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl TryFrom<Role> for PlayerRole {
    type Error = RecordsErrorKind;

    fn try_from(role: Role) -> Result<Self, Self::Error> {
        if role.id < 3 {
            // SAFETY: enum is repr(u8) and role id is in range
            Ok(unsafe { std::mem::transmute::<u8, PlayerRole>(role.id) })
        } else {
            Err(RecordsErrorKind::UnknownRole(role.id, role.role_name))
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

    async fn banishments(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(
            sqlx::query_as("SELECT * FROM banishments WHERE player_id = ?")
                .bind(self.inner.id)
                .fetch_all(db)
                .await?,
        )
    }

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<PlayerRole> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let r: Role = sqlx::query_as("SELECT * FROM role WHERE id = ?")
            .bind(self.inner.role)
            .fetch_one(db)
            .await?;

        Ok(r.try_into()?)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = DatabaseConnection::from(db.mysql_pool.clone());
        let mut redis_conn = db.redis_pool.get().await?;

        records_lib::assert_future_send(transaction::within(&conn, async |txn| {
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

pub struct PlayerLoader(pub MySqlPool);

impl Loader<u32> for PlayerLoader {
    type Value = Player;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = players::Entity::find()
            .filter(players::Column::Id.is_in(keys.iter().copied()))
            .all(&DatabaseConnection::from(self.0.clone()))
            .await?
            .into_iter()
            .map(|player| (player.id, player.into()))
            .collect();

        Ok(hashmap)
    }
}
