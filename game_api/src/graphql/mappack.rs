use std::time::SystemTime;

use async_graphql::SimpleObject;
use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    map,
    mappack::{update_mappack, AnyMappackId},
    must, player,
    redis_key::{
        mappack_key, mappack_lb_key, mappack_map_last_rank, mappack_mx_created_key,
        mappack_mx_name_key, mappack_mx_username_key, mappack_nb_map_key,
        mappack_player_map_finished_key, mappack_player_rank_avg_key, mappack_player_ranks_key,
        mappack_player_worst_rank_key, mappack_time_key,
    },
    Database, DatabaseConnection, MySqlPool, RedisPool,
};
use reqwest::Client;
use serde::Deserialize;

use crate::{RecordsErrorKind, RecordsResult, RecordsResultExt};

use super::{map::Map, player::Player};

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct MXMappackInfoResponse {
    Username: String,
    Name: String,
    Created: String,
}

async fn fill_mappack(
    client: &Client,
    conn: &mut DatabaseConnection,
    mappack: AnyMappackId<'_>,
    mappack_id: u32,
) -> RecordsResult<()> {
    let maps = map::fetch_mx_mappack_maps(client, mappack_id, None);

    let info = async {
        let res: MXMappackInfoResponse = client
            .get(format!(
                "https://sm.mania.exchange/api/mappack/get_info/{mappack_id}"
            ))
            .header("User-Agent", "obstacle (discord @ahmadbky)")
            .send()
            .await
            .with_api_err()?
            .json()
            .await
            .with_api_err()?;
        RecordsResult::Ok(res)
    };

    let (maps, info) = tokio::join!(maps, info);
    let (maps, info) = (maps?, info?);

    for mx_map in maps {
        // We check that the map exists in our database
        let _ = must::have_map(&mut conn.mysql_conn, &mx_map.TrackUID).await?;
        let _: () = conn
            .redis_conn
            .sadd(mappack_key(mappack), mx_map.TrackUID)
            .await
            .with_api_err()?;
    }

    // --------
    // These keys would probably be null for some mappacks, because they would belong
    // to an event edition, so these info would be retrieved from our information system.

    let _: () = conn
        .redis_conn
        .set(mappack_mx_username_key(mappack), info.Username)
        .await
        .with_api_err()?;

    let _: () = conn
        .redis_conn
        .set(mappack_mx_name_key(mappack), info.Name)
        .await
        .with_api_err()?;

    let _: () = conn
        .redis_conn
        .set(mappack_mx_created_key(mappack), info.Created)
        .await
        .with_api_err()?;

    Ok(())
}

pub struct Mappack {
    pub(crate) event_has_expired: bool,
    pub(crate) mappack_id: String,
}

impl From<String> for Mappack {
    fn from(mappack_id: String) -> Self {
        Self {
            mappack_id,
            event_has_expired: false,
        }
    }
}

#[derive(SimpleObject)]
struct MappackMap {
    rank: i32,
    last_rank: i32,
    map: Map,
}

struct MappackPlayer<'a> {
    mappack: &'a Mappack,
    inner: Player,
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
        let mut conn = db.acquire().await?;

        let maps_uids: Vec<String> = conn
            .redis_conn
            .zrange_withscores(
                mappack_player_ranks_key(
                    AnyMappackId::Id(&self.mappack.mappack_id),
                    self.inner.inner.id,
                ),
                0,
                -1,
            )
            .await?;
        let maps_uids = maps_uids.chunks_exact(2);

        let mut out = Vec::with_capacity(maps_uids.size_hint().0);

        for chunk in maps_uids {
            let [game_id, rank] = chunk else {
                unreachable!("plz stabilize Iterator::array_chunks")
            };
            let rank = rank.parse()?;
            let last_rank = conn
                .redis_conn
                .get(mappack_map_last_rank(
                    AnyMappackId::Id(&self.mappack.mappack_id),
                    game_id,
                ))
                .await?;
            let map = must::have_map(&mut conn.mysql_conn, game_id).await?;

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

#[async_graphql::Object]
impl Mappack {
    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let nb_map = redis_conn
            .get(mappack_nb_map_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(nb_map)
    }

    async fn mx_author(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<String>> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let author = redis_conn
            .get(mappack_mx_username_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(author)
    }

    async fn mx_created_at(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<chrono::NaiveDateTime>> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let created_at: Option<String> = redis_conn
            .get(mappack_mx_created_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(created_at.map(|s| s.parse()).transpose()?)
    }

    async fn mx_name(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<String>> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let name = redis_conn
            .get(mappack_mx_name_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(name)
    }

    async fn leaderboard<'a>(
        &'a self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<MappackPlayer<'a>>> {
        let db = ctx.data_unchecked::<Database>();
        let mut conn = db.acquire().await?;

        let leaderboard: Vec<u32> = conn
            .redis_conn
            .zrange(mappack_lb_key(AnyMappackId::Id(&self.mappack_id)), 0, -1)
            .await?;

        let mut out = Vec::with_capacity(leaderboard.len());

        for id in leaderboard {
            let player = player::get_player_from_id(&mut *conn.mysql_conn, id).await?;
            out.push(MappackPlayer {
                inner: player.into(),
                mappack: self,
            });
        }

        Ok(out)
    }

    async fn player<'a>(
        &'a self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<MappackPlayer<'a>> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = mysql_pool.acquire().await?;
        let player = must::have_player(&mut mysql_conn, &login).await?;

        Ok(MappackPlayer {
            inner: player.into(),
            mappack: self,
        })
    }

    async fn next_update_in(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<u64>> {
        if self.event_has_expired {
            return Ok(None);
        }

        let db = ctx.data_unchecked::<Database>();
        let redis_conn = &mut db.redis_pool.get().await?;
        let last_upd_time: Option<u64> = redis_conn
            .get(mappack_time_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(last_upd_time.map(|last| last + 24 * 3600).and_then(|last| {
            SystemTime::UNIX_EPOCH
                .elapsed()
                .ok()
                .and_then(|d| last.checked_sub(d.as_secs()))
        }))
    }
}

pub async fn get_mappack(
    ctx: &async_graphql::Context<'_>,
    mappack_id: String,
) -> RecordsResult<Mappack> {
    let db = ctx.data_unchecked::<Database>();
    let mut conn = db.acquire().await.with_api_err()?;
    let mappack = AnyMappackId::Id(&mappack_id);

    let mappack_uids: Vec<String> = conn
        .redis_conn
        .smembers(mappack_key(mappack))
        .await
        .with_api_err()?;

    // We load the campaign, and update it, before retrieving the scores from it
    if mappack_uids.is_empty() {
        let Ok(mappack_id_int) = mappack_id.parse() else {
            return Err(RecordsErrorKind::InvalidMappackId(mappack_id));
        };

        let client = ctx.data_unchecked::<Client>();

        // We fill the mappack
        fill_mappack(client, &mut conn, mappack, mappack_id_int).await?;

        // And we update it to have its scores cached
        update_mappack(AnyMappackId::Id(&mappack_id), &mut conn)
            .await
            .with_api_err()?;
    }

    Ok(From::from(mappack_id))
}
