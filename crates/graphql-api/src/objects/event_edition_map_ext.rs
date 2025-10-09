use deadpool_redis::redis::AsyncCommands as _;
use records_lib::{
    RedisPool, event as event_utils, mappack::AnyMappackId, redis_key::mappack_map_last_rank,
};
use sea_orm::DbConn;

use crate::{
    error::GqlResult,
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
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;

        let last_rank = redis_conn
            .get(mappack_map_last_rank(
                AnyMappackId::Event(
                    &self.edition_player.edition.event.inner,
                    &self.edition_player.edition.inner,
                ),
                &self.inner.inner.game_id,
            ))
            .await?;

        Ok(last_rank)
    }

    async fn medal_times(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<MedalTimes>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let medal_times = event_utils::get_medal_times_of(
            conn,
            self.edition_player.edition.inner.event_id,
            self.edition_player.edition.inner.id,
            self.inner.inner.id,
        )
        .await?;

        Ok(medal_times.map(From::from))
    }
}
