use deadpool_redis::redis::AsyncCommands as _;
use records_lib::{
    Database, RedisPool,
    mappack::AnyMappackId,
    must,
    redis_key::{
        mappack_lb_key, mappack_map_last_rank, mappack_player_map_finished_key,
        mappack_player_rank_avg_key, mappack_player_ranks_key, mappack_player_worst_rank_key,
    },
};

use crate::objects::{mappack::Mappack, mappack_map::MappackMap, player::Player};

pub struct MappackPlayer<'a> {
    pub mappack: &'a Mappack,
    pub inner: Player,
}

pub(super) async fn player_rank(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> async_graphql::Result<usize> {
    let redis_pool = ctx.data_unchecked::<RedisPool>();
    let redis_conn = &mut redis_pool.get().await?;
    let rank = redis_conn
        .zscore(mappack_lb_key(mappack), player_id)
        .await?;
    Ok(rank)
}

pub(super) async fn player_rank_avg(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> async_graphql::Result<f64> {
    let redis_pool = ctx.data_unchecked::<RedisPool>();
    let redis_conn = &mut redis_pool.get().await?;
    let rank = redis_conn
        .get(mappack_player_rank_avg_key(mappack, player_id))
        .await?;
    Ok(rank)
}

pub(super) async fn player_map_finished(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> async_graphql::Result<usize> {
    let redis_pool = ctx.data_unchecked::<RedisPool>();
    let redis_conn = &mut redis_pool.get().await?;
    let map_finished = redis_conn
        .get(mappack_player_map_finished_key(mappack, player_id))
        .await?;
    Ok(map_finished)
}

pub(super) async fn player_worst_rank(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> async_graphql::Result<i32> {
    let redis_pool = ctx.data_unchecked::<RedisPool>();
    let redis_conn = &mut redis_pool.get().await?;
    let worst_rank = redis_conn
        .get(mappack_player_worst_rank_key(mappack, player_id))
        .await?;
    Ok(worst_rank)
}

#[async_graphql::Object]
impl MappackPlayer<'_> {
    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        player_rank(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn player(&self) -> &Player {
        &self.inner
    }

    async fn ranks(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<MappackMap>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        let maps_uids: Vec<String> = redis_conn
            .zrange_withscores(
                mappack_player_ranks_key(
                    AnyMappackId::Id(&self.mappack.mappack_id),
                    self.inner.inner.id,
                ),
                0,
                -1,
            )
            .await?;
        let (maps_uids, _) = maps_uids.as_chunks::<2>();

        let mut out = Vec::with_capacity(maps_uids.len());

        for [game_id, rank] in maps_uids {
            let rank = rank.parse()?;
            let last_rank = redis_conn
                .get(mappack_map_last_rank(
                    AnyMappackId::Id(&self.mappack.mappack_id),
                    game_id,
                ))
                .await?;
            let map = must::have_map(&db.sql_conn, game_id).await?;

            out.push(MappackMap {
                map: map.into(),
                rank,
                last_rank,
            });
        }

        Ok(out)
    }

    async fn rank_avg(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<f64> {
        player_rank_avg(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn map_finished(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        player_map_finished(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn worst_rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i32> {
        player_worst_rank(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }
}
