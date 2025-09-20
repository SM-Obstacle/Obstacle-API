use deadpool_redis::redis::AsyncCommands as _;
use records_lib::{RedisPool, mappack::AnyMappackId, must, redis_key::mappack_player_ranks_key};
use sea_orm::DbConn;

use crate::objects::{
    event_edition_map_ext::EventEditionMapExt, event_edition_player::EventEditionPlayer,
};

pub struct EventEditionPlayerRank<'a> {
    pub edition_player: &'a EventEditionPlayer<'a>,
    pub map_game_id: String,
    pub record_time: i32,
}

#[async_graphql::Object]
impl EventEditionPlayerRank<'_> {
    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let rank = redis_conn
            .zscore(
                mappack_player_ranks_key(
                    AnyMappackId::Event(
                        &self.edition_player.edition.event.inner,
                        &self.edition_player.edition.inner,
                    ),
                    self.edition_player.player.id,
                ),
                &self.map_game_id,
            )
            .await?;
        Ok(rank)
    }

    async fn time(&self) -> i32 {
        self.record_time
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<EventEditionMapExt<'_>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map = must::have_map(conn, &self.map_game_id).await?;
        Ok(EventEditionMapExt {
            inner: map.into(),
            edition_player: self.edition_player,
        })
    }
}
