use async_graphql::{Context, dataloader::DataLoader};
use entity::{checkpoint_times, records};
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait, QueryFilter, QueryOrder, QuerySelect, prelude::Expr,
    sea_query::Func,
};

use crate::internal;

use super::{
    map::{Map, MapLoader},
    player::{Player, PlayerLoader},
};

#[derive(Debug, Clone)]
pub struct RankedRecord {
    inner: records::RankedRecord,
}

impl From<records::RankedRecord> for RankedRecord {
    fn from(inner: records::RankedRecord) -> Self {
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
    ) -> async_graphql::Result<Vec<checkpoint_times::Model>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let times = checkpoint_times::Entity::find()
            .filter(checkpoint_times::Column::MapId.eq(self.inner.record.map_id))
            .group_by(checkpoint_times::Column::CpNum)
            .order_by_asc(checkpoint_times::Column::CpNum)
            .select_only()
            .columns([
                checkpoint_times::Column::CpNum,
                checkpoint_times::Column::MapId,
                checkpoint_times::Column::RecordId,
            ])
            .expr_as(
                Func::cust("FLOOR").arg(Func::avg(Expr::col(checkpoint_times::Column::Time))),
                "time",
            )
            .into_model()
            .all(conn)
            .await?;

        Ok(times)
    }

    async fn cps_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<checkpoint_times::Model>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let times = checkpoint_times::Entity::find()
            .filter(
                checkpoint_times::Column::RecordId
                    .eq(self.inner.record.record_id)
                    .and(checkpoint_times::Column::MapId.eq(self.inner.record.map_id)),
            )
            .order_by_asc(checkpoint_times::Column::CpNum)
            .all(conn)
            .await?;

        Ok(times)
    }

    async fn time(&self) -> i32 {
        self.inner.record.time
    }

    async fn respawn_count(&self) -> i32 {
        self.inner.record.respawn_count
    }

    async fn try_count(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i32> {
        let conn = ctx.data_unchecked::<DbConn>();
        let sum: Option<_> = records::Entity::find()
            .filter(
                records::Column::RecordPlayerId
                    .eq(self.inner.record.record_player_id)
                    .and(records::Column::MapId.eq(self.inner.record.map_id)),
            )
            .select_only()
            .expr(Func::cast_as(records::Column::TryCount.sum(), "INT"))
            .into_tuple()
            .one(conn)
            .await?
            .ok_or_else(|| {
                internal!(
                    "Record of player {} on map {} must exist in database",
                    self.inner.record.record_player_id,
                    self.inner.record.map_id
                )
            })?;

        Ok(sum.unwrap_or(1))
    }

    async fn record_date(&self) -> chrono::DateTime<chrono::Utc> {
        self.inner.record.record_date.and_utc()
    }

    async fn flags(&self) -> u32 {
        self.inner.record.flags
    }
}
