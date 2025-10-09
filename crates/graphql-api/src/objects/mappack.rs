use std::time::SystemTime;

use deadpool_redis::redis::AsyncCommands as _;
use records_lib::{
    Database, RedisConnection, RedisPool,
    error::{RecordsError, RecordsResult},
    internal, map,
    mappack::{AnyMappackId, update_mappack},
    must, player,
    redis_key::{
        mappack_key, mappack_lb_key, mappack_mx_created_key, mappack_mx_name_key,
        mappack_mx_username_key, mappack_nb_map_key, mappack_time_key,
    },
};
use sea_orm::{ConnectionTrait, DbConn};

use crate::{error::GqlResult, objects::mappack_player::MappackPlayer};

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct MXMappackInfoResponse {
    Username: String,
    Name: String,
    Created: String,
}

async fn fill_mappack<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    client: &reqwest::Client,
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
            .await?
            .json()
            .await?;
        RecordsResult::Ok(res)
    };

    let (maps, info) = tokio::join!(maps, info);
    let (maps, info) = (maps?, info?);

    for mx_map in maps {
        // We check that the map exists in our database
        let _ = must::have_map(conn, &mx_map.TrackUID).await?;
        let _: () = redis_conn
            .sadd(mappack_key(mappack), mx_map.TrackUID)
            .await?;
    }

    // --------
    // These keys would probably be null for some mappacks, because they would belong
    // to an event edition, so these info would be retrieved from our information system.

    let _: () = redis_conn
        .set(mappack_mx_username_key(mappack), info.Username)
        .await?;

    let _: () = redis_conn
        .set(mappack_mx_name_key(mappack), info.Name)
        .await?;

    let _: () = redis_conn
        .set(mappack_mx_created_key(mappack), info.Created)
        .await?;

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

#[async_graphql::Object]
impl Mappack {
    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let nb_map = redis_conn
            .get(mappack_nb_map_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(nb_map)
    }

    async fn mx_author(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<String>> {
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
    ) -> GqlResult<Option<chrono::NaiveDateTime>> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let created_at: Option<String> = redis_conn
            .get(mappack_mx_created_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        let parsed_date = created_at
            .map(|s| {
                s.parse().map_err(|e| {
                    internal!(
                        "create timestamp of mappack {} is an invalid timestamp: {e}. got `{s}`",
                        self.mappack_id
                    )
                })
            })
            .transpose()?;

        Ok(parsed_date)
    }

    async fn mx_name(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<String>> {
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
    ) -> GqlResult<Vec<MappackPlayer<'a>>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        let leaderboard: Vec<u32> = redis_conn
            .zrange(mappack_lb_key(AnyMappackId::Id(&self.mappack_id)), 0, -1)
            .await?;

        let mut out = Vec::with_capacity(leaderboard.len());

        for id in leaderboard {
            let player = player::get_player_from_id(&db.sql_conn, id).await?;
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
    ) -> GqlResult<MappackPlayer<'a>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let player = must::have_player(conn, &login).await?;

        Ok(MappackPlayer {
            inner: player.into(),
            mappack: self,
        })
    }

    async fn next_update_in(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<u64>> {
        if self.event_has_expired {
            return Ok(None);
        }

        let db = ctx.data_unchecked::<Database>();
        let redis_conn = &mut db.redis_pool.get().await?;
        let last_upd_time: Option<u64> = redis_conn
            .get(mappack_time_key(AnyMappackId::Id(&self.mappack_id)))
            .await?;
        Ok(last_upd_time
            .map(|last| last + records_lib::env().event_scores_interval.as_secs())
            .and_then(|last| {
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
    let conn = ctx.data_unchecked::<DbConn>();
    let mut redis_conn = db.redis_pool.get().await?;

    let mappack = AnyMappackId::Id(&mappack_id);

    let mappack_uids: Vec<String> = redis_conn.smembers(mappack_key(mappack)).await?;

    // We load the campaign, and update it, before retrieving the scores from it
    if mappack_uids.is_empty() {
        let Ok(mappack_id_int) = mappack_id.parse() else {
            return Err(RecordsError::InvalidMappackId(mappack_id));
        };

        let client = ctx.data_unchecked::<reqwest::Client>();

        // We fill the mappack
        fill_mappack(conn, &mut redis_conn, client, mappack, mappack_id_int).await?;

        // And we update it to have its scores cached
        update_mappack(
            conn,
            &mut redis_conn,
            AnyMappackId::Id(&mappack_id),
            Default::default(),
        )
        .await?;
    }

    Ok(From::from(mappack_id))
}
