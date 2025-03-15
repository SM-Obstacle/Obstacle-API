use async_graphql::{dataloader::DataLoader, Context};
use records_lib::{
    models::{self, CheckpointTime},
    Database, MySqlPool,
};

use super::{
    map::{Map, MapLoader},
    player::{Player, PlayerLoader},
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RankedRecord {
    #[sqlx(flatten)]
    inner: models::RankedRecord,
}

impl From<models::RankedRecord> for RankedRecord {
    fn from(inner: models::RankedRecord) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl RankedRecord {
    async fn id(&self) -> u32 {
        self.inner.record.record_id
    }

    async fn rank(&self) -> i32 {
        self.inner.rank
    }

    async fn map(&self, ctx: &Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<DataLoader<MapLoader>>()
            .load_one(self.inner.record.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
    }

    async fn player(&self, ctx: &Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.record.record_player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))
    }

    async fn average_cps_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<CheckpointTime>> {
        let db = &ctx.data_unchecked::<Database>().mysql_pool;

        Ok(sqlx::query_as(
            "SELECT cp_num, map_id, record_id, FLOOR(AVG(time)) AS time
            FROM checkpoint_times
            WHERE map_id = ?
            GROUP BY cp_num
            ORDER BY cp_num",
        )
        .bind(self.inner.record.map_id)
        .fetch_all(db)
        .await?)
    }

    async fn cps_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<CheckpointTime>> {
        let db = &ctx.data_unchecked::<Database>().mysql_pool;

        Ok(sqlx::query_as(
            "SELECT * FROM checkpoint_times WHERE record_id = ? AND map_id = ? ORDER BY cp_num",
        )
        .bind(self.inner.record.record_id)
        .bind(self.inner.record.map_id)
        .fetch_all(db)
        .await?)
    }

    async fn time(&self) -> i32 {
        self.inner.record.time
    }

    async fn respawn_count(&self) -> i32 {
        self.inner.record.respawn_count
    }

    async fn try_count(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i32> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let sum: Option<i32> = sqlx::query_scalar(
            "select cast(sum(try_count) as int) from records
            where record_player_id = ? and map_id = ?",
        )
        .bind(self.inner.record.record_player_id)
        .bind(self.inner.record.map_id)
        .fetch_one(db)
        .await?;
        Ok(sum.unwrap_or(1))
    }

    async fn record_date(&self) -> chrono::DateTime<chrono::Utc> {
        self.inner.record.record_date.and_utc()
    }

    async fn flags(&self) -> u32 {
        self.inner.record.flags
    }
}
