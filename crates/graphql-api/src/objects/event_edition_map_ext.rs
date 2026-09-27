use async_graphql::dataloader::DataLoader;
use entity::mappack_map_periodic_ranking;
use records_lib::{event as event_utils, mappack::AnyMappackId};
use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _, QueryOrder as _};

use crate::{
    error::GqlResult,
    loaders::event_edition_map::EventEditionMapLoader,
    objects::{event_edition_player::EventEditionPlayer, map::Map, medal_times::MedalTimes},
};

pub struct EventEditionMapExt<'a> {
    pub edition_player: &'a EventEditionPlayer<'a>,
    pub inner: Map,
}

#[async_graphql::Object]
impl EventEditionMapExt<'_> {
    async fn map(&self) -> &Map {
        &self.inner
    }

    async fn last_rank(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<i32> {
        let db = ctx.data_unchecked::<records_lib::Database>();
        let snapshot = super::mappack_player::latest_snapshot(
            &db.sql_conn,
            AnyMappackId::Event(
                &self.edition_player.edition.event.inner,
                &self.edition_player.edition.inner,
            ),
        )
        .await?;
        let Some(snapshot) = snapshot else {
            return Ok(0);
        };
        Ok(mappack_map_periodic_ranking::Entity::find()
            .filter(
                mappack_map_periodic_ranking::Column::PeriodId
                    .eq(snapshot.period_id)
                    .and(mappack_map_periodic_ranking::Column::MappackId.eq(snapshot.mappack_id))
                    .and(mappack_map_periodic_ranking::Column::MapId.eq(self.inner.inner.id)),
            )
            .order_by_desc(mappack_map_periodic_ranking::Column::LastRank)
            .one(&db.sql_conn)
            .await?
            .map_or(0, |score| score.last_rank as i32))
    }

    async fn medal_times(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<MedalTimes>> {
        let key = (
            self.edition_player.edition.inner.event_id,
            self.edition_player.edition.inner.id,
            self.inner.inner.id,
        );

        let Some(row) = ctx
            .data_unchecked::<DataLoader<EventEditionMapLoader>>()
            .load_one(key)
            .await?
        else {
            return Ok(None);
        };

        let medal_times = event_utils::MedalTimes::from_columns(
            row.bronze_time,
            row.silver_time,
            row.gold_time,
            row.author_time,
        );

        Ok(medal_times.map(From::from))
    }
}
