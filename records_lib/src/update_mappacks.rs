use std::{fmt, time::SystemTime};

use async_graphql::SimpleObject;
use deadpool_redis::redis::{AsyncCommands, SetExpiry, SetOptions, ToRedisArgs};
use sqlx::MySqlConnection;

use crate::{
    error::RecordsResult,
    escaped::Escaped,
    models, must,
    redis_key::{
        mappack_key, mappack_lb_key, mappack_map_last_rank, mappack_nb_map_key,
        mappack_player_map_finished_key, mappack_player_rank_avg_key, mappack_player_ranks_key,
        mappack_player_worst_rank_key, mappack_time_key, mappacks_key,
    },
    update_ranks::get_rank_or_full_update,
    GetSqlFragments, RedisConnection,
};

#[derive(SimpleObject, Default, Clone, Debug)]
pub struct Rank {
    pub rank: i32,
    pub map_idx: usize,
}

#[derive(SimpleObject, Debug)]
pub struct PlayerScore {
    pub player_id: u32,
    pub login: String,
    pub name: Escaped,
    pub ranks: Vec<Rank>,
    pub score: f64,
    pub maps_finished: usize,
    pub rank: u32,
    pub worst: Rank,
}

#[derive(SimpleObject, Debug)]
pub struct MappackMap {
    pub map: Escaped,
    pub map_id: Escaped,
    pub last_rank: i32,
    #[graphql(skip)]
    pub records: Option<Vec<RankedRecordRow>>,
}

#[derive(SimpleObject)]
struct PlayerScoresDetails {
    pub a: i32,
}

#[derive(Debug)]
pub struct MappackScores {
    pub maps: Vec<MappackMap>,
    pub scores: Vec<PlayerScore>,
}

#[derive(sqlx::FromRow, Debug)]
struct RecordRow {
    #[sqlx(flatten)]
    pub record: models::Record,
    pub player_id2: u32,
    pub player_login: String,
    #[sqlx(try_from = "String")]
    pub player_name: Escaped,
}

#[derive(Debug)]
pub struct RankedRecordRow {
    rank: i32,
    record: RecordRow,
}

#[derive(Clone, Copy)]
pub enum MappackKind<'a> {
    Event(&'a models::Event, &'a models::EventEdition),
    Id(&'a str),
}

impl fmt::Debug for MappackKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.mappack_id(), f)
    }
}

pub struct MappackIdDisp<'a, 'b> {
    kind: &'a MappackKind<'b>,
}

impl fmt::Display for MappackIdDisp<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            MappackKind::Event(_, edition) => {
                if let Some(id) = edition.mx_id {
                    fmt::Display::fmt(&id, f)
                } else {
                    write!(f, "__{}__{}__", edition.event_id, edition.id)
                }
            }
            MappackKind::Id(id) => f.write_str(id),
        }
    }
}

impl ToRedisArgs for MappackIdDisp<'_, '_> {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + deadpool_redis::redis::RedisWrite,
    {
        out.write_arg_fmt(self)
    }
}

impl MappackKind<'_> {
    pub fn mappack_id(&self) -> MappackIdDisp<'_, '_> {
        MappackIdDisp { kind: self }
    }

    fn to_opt_event(&self) -> Option<(&models::Event, &models::EventEdition)> {
        match self {
            Self::Event(event, edition) => Some((event, edition)),
            Self::Id(_) => None,
        }
    }

    fn has_ttl(&self) -> bool {
        matches!(self, Self::Id(_))
    }

    fn get_ttl(&self) -> Option<i64> {
        self.has_ttl().then_some(crate::env().mappack_ttl)
    }
}

#[cfg_attr(
    feature = "tracing",
    tracing::instrument(skip(mysql_conn, redis_conn), err, ret)
)]
pub async fn update_mappack(
    mappack: MappackKind<'_>,
    mysql_conn: &mut MySqlConnection,
    redis_conn: &mut RedisConnection,
) -> RecordsResult<()> {
    // Calculate the scores
    let scores = calc_scores(mappack, mysql_conn, redis_conn).await?;

    // Early return if the mappack has expired
    let Some(scores) = scores else {
        return Ok(());
    };

    // Then save them to the Redis database for cache-handling
    save(mappack, scores, redis_conn).await?;

    // And we save it to the registered mappacks set.
    if mappack.has_ttl() {
        // The mappack has a TTL, so its member will be removed from the set when
        // attempting to retrieve its maps.
        redis_conn
            .sadd(mappacks_key(), mappack.mappack_id())
            .await?;
    }

    Ok(())
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip(scores, redis_conn)))]
async fn save(
    mappack: MappackKind<'_>,
    scores: MappackScores,
    redis_conn: &mut RedisConnection,
) -> RecordsResult<()> {
    let set_options = SetOptions::default();
    let set_options = match mappack.get_ttl() {
        Some(ex) => {
            // Update expiration time of some keys btw
            redis_conn.expire(mappack_key(mappack), ex).await?;
            redis_conn.expire(mappack_lb_key(mappack), ex).await?;

            set_options.with_expiration(SetExpiry::EX(ex as _))
        }
        None => {
            // Persist some keys btw
            redis_conn.persist(mappack_key(mappack)).await?;
            redis_conn.persist(mappack_lb_key(mappack)).await?;

            set_options
        }
    };

    // --- Save the number of maps of the campaign

    let key = mappack_nb_map_key(mappack);
    redis_conn
        .set_options(&key, scores.maps.len(), set_options)
        .await?;

    if !mappack.has_ttl() {
        redis_conn.persist(&key).await?;
    }

    for map in &scores.maps {
        // --- Save the last rank on each map

        redis_conn
            .set_options(
                mappack_map_last_rank(mappack, &map.map_id),
                map.last_rank,
                set_options,
            )
            .await?;

        if !mappack.has_ttl() {
            redis_conn
                .persist(mappack_map_last_rank(mappack, &map.map_id))
                .await?;
        }
    }

    for score in scores.scores {
        redis_conn
            .zadd(mappack_lb_key(mappack), score.player_id, score.rank)
            .await?;

        // --- Save the rank average

        let rank_avg = ((score.score + f64::EPSILON) * 100.).round() / 100.;

        redis_conn
            .set_options(
                mappack_player_rank_avg_key(mappack, score.player_id),
                rank_avg,
                set_options,
            )
            .await?;

        // --- Save the amount of finished map

        redis_conn
            .set_options(
                mappack_player_map_finished_key(mappack, score.player_id),
                score.maps_finished,
                set_options,
            )
            .await?;

        // --- Save their worst rank

        redis_conn
            .set_options(
                mappack_player_worst_rank_key(mappack, score.player_id),
                score.worst.rank,
                set_options,
            )
            .await?;

        if let Some(ttl) = mappack.get_ttl() {
            redis_conn
                .expire(mappack_player_ranks_key(mappack, score.player_id), ttl)
                .await?;
        } else {
            redis_conn
                .persist(mappack_player_ranks_key(mappack, score.player_id))
                .await?;
            redis_conn
                .persist(mappack_player_rank_avg_key(mappack, score.player_id))
                .await?;
            redis_conn
                .persist(mappack_player_map_finished_key(mappack, score.player_id))
                .await?;
            redis_conn
                .persist(mappack_player_worst_rank_key(mappack, score.player_id))
                .await?;
        }

        for (game_id, rank) in score
            .ranks
            .into_iter()
            .map(|rank| (&scores.maps[rank.map_idx].map_id, rank.rank))
        {
            // --- Save their rank on each map

            redis_conn
                .zadd(
                    mappack_player_ranks_key(mappack, score.player_id),
                    game_id,
                    rank,
                )
                .await?;
        }
    }

    if let Ok(time) = SystemTime::UNIX_EPOCH.elapsed() {
        redis_conn
            .set(mappack_time_key(mappack), time.as_secs())
            .await?;
        if let Some(ttl) = mappack.get_ttl() {
            redis_conn.expire(mappack_time_key(mappack), ttl).await?;
        } else {
            redis_conn.persist(mappack_time_key(mappack)).await?;
        }
    }

    Ok(())
}

/// Returns an `Option` because the mappack may have expired.
#[cfg_attr(feature = "tracing", tracing::instrument(skip(mysql_conn, redis_conn)))]
async fn calc_scores(
    mappack: MappackKind<'_>,
    mysql_conn: &mut MySqlConnection,
    redis_conn: &mut RedisConnection,
) -> RecordsResult<Option<MappackScores>> {
    let mappack_key = mappack_key(mappack);
    let mappack_uids: Vec<String> = redis_conn.smembers(&mappack_key).await?;

    let mut maps = Vec::with_capacity(mappack_uids.len().max(5));

    let event = mappack.to_opt_event();
    let (join_event, and_event) = event.get_sql_fragments();

    let mappack = if mappack_uids.is_empty() {
        // If the mappack is empty, it means either that it's an invalid/unknown mappack ID,
        // or that its TTL has expired. So we remove its entry in the registered mappacks set.
        // The other keys related to this mappack were set with a TTL so they should
        // be deleted too.
        let _: i32 = redis_conn
            .srem(mappacks_key(), mappack.mappack_id())
            .await?;
        return Ok(None);
    } else {
        let mut out = Vec::with_capacity(mappack_uids.len());
        for map_uid in &mappack_uids {
            let map = must::have_map(&mut *mysql_conn, map_uid).await?;
            maps.push(MappackMap {
                map: map.name.clone().into(),
                map_id: map.game_id.clone().into(),
                last_rank: 0,
                records: None,
            });
            out.push(map);
        }
        out
    };

    let mut scores = Vec::<PlayerScore>::with_capacity(mappack.len());

    for (i, map) in mappack.iter().enumerate() {
        let query = format!(
            "SELECT r.*, p.id as player_id2, p.login as player_login, p.name as player_name
            FROM global_records r
            {join_event}
            INNER JOIN players p ON p.id = r.record_player_id
            WHERE map_id = ?
            {and_event}
            ORDER BY time ASC",
        );

        let query = sqlx::query_as::<_, RecordRow>(&query).bind(map.id);
        let query = if let Some((event, edition)) = event {
            query.bind(event.id).bind(edition.id)
        } else {
            query
        };

        let res = query.fetch_all(&mut *mysql_conn).await?;

        let mut records = Vec::with_capacity(res.len());

        for record in res {
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
                    map.id,
                    record.record.time,
                    None,
                )
                .await?,
                record,
            };
            records.push(record);
        }

        maps[i].records = Some(records);
    }

    let mut map_number = 1;

    for (map_idx, map) in maps.iter_mut().enumerate() {
        let records = map.records.take().unwrap();

        let last_rank = records.iter().map(|p| p.rank).max().unwrap_or(0);

        for record in records {
            let player = scores
                .iter_mut()
                .find(|p| p.player_id == record.record.player_id2)
                .unwrap();

            player.ranks.push(Rank {
                rank: record.rank,
                map_idx,
            });
            map.last_rank = last_rank;

            player.maps_finished += 1;
        }

        for player in &mut scores {
            if player.ranks.len() < map_number {
                player.ranks.push(Rank {
                    rank: last_rank + 1,
                    map_idx,
                });
                map.last_rank = last_rank;
            }
        }

        map_number += 1;
    }

    for player in &mut scores {
        player.ranks.sort_by(|a, b| {
            ((a.rank / maps[a.map_idx].last_rank.max(1)
                - b.rank / maps[b.map_idx].last_rank.max(1))
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

    Ok(Some(MappackScores { maps, scores }))
}
