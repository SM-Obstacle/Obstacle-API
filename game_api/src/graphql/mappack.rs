use std::collections::HashMap;

use async_graphql::SimpleObject;
use deadpool_redis::{redis::AsyncCommands, Connection as RedisConnection};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sqlx::{FromRow, MySqlConnection};

use crate::{
    get_env_var_as, models, must, utils::format_mappack_key, Database, RecordsErrorKind,
    RecordsResult,
};

use super::{get_rank_or_full_update, utils::RecordAttr};

#[derive(SimpleObject, Default, Clone, Debug)]
pub struct Rank {
    rank: i32,
    map_idx: usize,
}

#[derive(SimpleObject, Debug)]
pub struct PlayerScore {
    player_id: u32,
    login: String,
    name: String,
    ranks: Vec<Rank>,
    score: f64,
    maps_finished: usize,
    rank: u32,
    worst: Rank,
}

#[derive(SimpleObject)]
pub struct MappackMap {
    map: String,
    map_id: String,
    last_rank: i32,
}

#[derive(SimpleObject)]
pub struct MappackScores {
    maps: Vec<MappackMap>,
    scores: Vec<PlayerScore>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct MXMappackResponseItem {
    TrackUID: String,
}

async fn load_campaign(
    client: &Client,
    db: &mut MySqlConnection,
    redis_conn: &mut RedisConnection,
    mappack_key: &str,
    mappack_id: u32,
) -> RecordsResult<Vec<models::Map>> {
    let res: Vec<MXMappackResponseItem> = client
        .get(format!(
            "https://sm.mania.exchange/api/mappack/get_mappack_tracks/{mappack_id}"
        ))
        .header("User-Agent", "obstacle (ahmadbky@5382)")
        .send()
        .await?
        .json()
        .await?;

    let mut c = Vec::with_capacity(res.len());

    for MXMappackResponseItem { TrackUID } in res {
        let map = must::have_map(&mut *db, &TrackUID).await?;
        c.push(map);
        redis_conn.sadd(mappack_key, TrackUID).await?;
    }

    Ok(c)
}

#[derive(FromRow)]
struct RecordRow {
    #[sqlx(flatten)]
    record: RecordAttr,
    player_id2: u32,
    player_login: String,
    player_name: String,
}

struct RankedRecordRow {
    rank: i32,
    record: RecordRow,
}

pub async fn calc_scores(
    ctx: &async_graphql::Context<'_>,
    mappack_id: String,
) -> RecordsResult<MappackScores> {
    let db = ctx.data_unchecked::<Database>();
    let mysql_conn = &mut db.mysql_pool.acquire().await?;
    let redis_conn = &mut db.redis_pool.get().await?;
    let mappack_key = format_mappack_key(&mappack_id);

    let mappack_uids: Vec<String> = redis_conn.smembers(&mappack_key).await?;

    let mut maps = Vec::with_capacity(mappack_uids.len().max(5));

    let mappack = if mappack_uids.is_empty() {
        let Ok(mappack_id) = mappack_id.parse() else {
            return Err(RecordsErrorKind::InvalidMappackId(mappack_id));
        };
        let client = ctx.data_unchecked::<Client>();
        let out = load_campaign(client, mysql_conn, redis_conn, &mappack_key, mappack_id).await?;
        for map in &out {
            maps.push(MappackMap {
                map: map.name.clone(),
                map_id: map.game_id.clone(),
                last_rank: 0,
            });
        }
        out
    } else {
        let mut out = Vec::with_capacity(mappack_uids.len());
        for map_uid in &mappack_uids {
            let map = must::have_map(&mut **mysql_conn, map_uid).await?;
            maps.push(MappackMap {
                map: map.name.clone(),
                map_id: map.game_id.clone(),
                last_rank: 0,
            });
            out.push(map);
        }
        out
    };

    let no_ttl: Vec<String> = redis_conn.smembers("v3:no_ttl_mappacks").await?;
    if !no_ttl.contains(&mappack_id) {
        // Update the expiration time of the redis key
        let mappack_ttl = get_env_var_as("RECORDS_API_MAPPACK_TTL");
        redis_conn.expire(mappack_key, mappack_ttl).await?;
    }

    let mut scores = Vec::<PlayerScore>::with_capacity(mappack.len());

    let mut hashmap = HashMap::new();

    for map in &mappack {
        let mut res = sqlx::query_as::<_, RecordRow>(
            "SELECT r.*, p.id as player_id2, p.login as player_login, p.name as player_name
            FROM global_records r
            INNER JOIN players p ON p.id = r.record_player_id
            WHERE map_id = ?
            ORDER BY time ASC",
        )
        .bind(map.id)
        .fetch(&mut **mysql_conn);

        let mysql_conn = &mut db.mysql_pool.acquire().await?;

        let mut records = Vec::with_capacity(res.size_hint().0);

        while let Some(record) = res.next().await {
            let record = record?;

            if !scores.iter().any(|p| p.player_id == record.player_id2) {
                scores.push(PlayerScore {
                    player_id: record.player_id2,
                    login: record.player_login.clone(),
                    name: record.player_name.clone(),
                    ranks: Vec::new(),
                    score: 0.,
                    maps_finished: 0,
                    rank: 0,
                    worst: Default::default(),
                });
            }

            let record = RankedRecordRow {
                rank: get_rank_or_full_update(
                    (&mut *mysql_conn, redis_conn),
                    map,
                    record.record.record.time,
                    None,
                )
                .await?,
                record,
            };
            records.push(record);
        }

        hashmap.insert(map.id, records);
    }

    let mut map_number = 1;

    for (map_idx, map) in mappack.iter().enumerate() {
        let records = hashmap
            .get(&map.id)
            .unwrap_or_else(|| panic!("map not in hashmap: {}", map.id));

        let last_rank = records.iter().map(|p| p.rank).max().unwrap_or(99);

        for record in records {
            let player = scores
                .iter_mut()
                .find(|p| p.player_id == record.record.player_id2)
                .unwrap();

            player.ranks.push(Rank {
                rank: record.rank,
                map_idx,
            });
            maps[map_idx].last_rank = last_rank;

            player.maps_finished += 1;
        }

        for player in &mut scores {
            if player.ranks.len() < map_number {
                player.ranks.push(Rank {
                    rank: last_rank + 1,
                    map_idx,
                });
                maps[map_idx].last_rank = last_rank;
            }
        }

        map_number += 1;
    }

    for player in &mut scores {
        player.ranks.sort_by(|a, b| {
            ((a.rank / maps[a.map_idx].last_rank - b.rank / maps[b.map_idx].last_rank)
                + (a.rank - b.rank) / 1000)
                .cmp(&0)
        });

        player.worst = player
            .ranks
            .iter()
            .reduce(|a, b| if a.rank > b.rank { a } else { b })
            .unwrap()
            .clone();

        player.score = player
            .ranks
            .iter()
            .fold(0., |acc, rank| acc + rank.rank as f64)
            / player.ranks.len() as f64;
    }

    scores.sort_by(|a, b| {
        if a.maps_finished != b.maps_finished {
            b.maps_finished.cmp(&a.maps_finished)
        } else {
            a.score.partial_cmp(&b.score).unwrap()
        }
    });

    let mut old_score = 0.;
    let mut old_finishes = 0;
    let mut old_rank = 0;

    for (rank, player) in scores.iter_mut().enumerate() {
        player.rank = if old_score.eq(&player.score) && old_finishes == player.maps_finished {
            old_rank
        } else {
            rank as u32 + 1
        };

        old_score = player.score;
        old_finishes = player.maps_finished;
        old_rank = player.rank;
    }

    Ok(MappackScores { maps, scores })
}
