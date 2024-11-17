//! This module contains anything related to mappacks in this library.

use std::{fmt, time::SystemTime};

use deadpool_redis::redis::{AsyncCommands, SetExpiry, SetOptions, ToRedisArgs};

use crate::{
    error::RecordsResult,
    event::OptEvent,
    models, must,
    ranks::get_rank,
    redis_key::{
        mappack_key, mappack_lb_key, mappack_map_last_rank, mappack_nb_map_key,
        mappack_player_map_finished_key, mappack_player_rank_avg_key, mappack_player_ranks_key,
        mappack_player_worst_rank_key, mappack_time_key, mappacks_key,
    },
    DatabaseConnection, RedisConnection,
};

#[derive(Default, Clone, Debug)]
struct Rank {
    rank: i32,
    map_idx: usize,
}

#[derive(Debug)]
struct PlayerScore {
    player_id: u32,
    ranks: Vec<Rank>,
    score: f64,
    maps_finished: usize,
    rank: u32,
    worst: Rank,
}

#[derive(Debug)]
struct MappackMap {
    map_id: String,
    last_rank: i32,
    records: Option<Vec<RankedRecordRow>>,
}

#[derive(Debug)]
struct MappackScores {
    maps: Vec<MappackMap>,
    scores: Vec<PlayerScore>,
}

#[derive(sqlx::FromRow, Debug)]
struct RecordRow {
    #[sqlx(flatten)]
    record: models::Record,
    player_id2: u32,
}

#[derive(Debug)]
struct RankedRecordRow {
    rank: i32,
    record: RecordRow,
}

/// Represents any mappack ID, meaning an event or a regular MX mappack.
///
/// If it is an event without an associated mappack, the mappack ID is `__X__Y__` where X
/// is the event ID and Y the edition ID. Otherwise, it is the ID of the associated mappack.
///
/// If it is a regular MX mappack, it is its ID.
#[derive(Clone, Copy)]
pub enum AnyMappackId<'a> {
    /// The mappack is related to an event.
    Event(&'a models::Event, &'a models::EventEdition),
    /// The mappack is a regular MX mappack.
    Id(&'a str),
}

impl fmt::Debug for AnyMappackId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.mappack_id(), f)
    }
}

/// Wrapper type of the [`AnyMappackId`] type to be displayed as a mappack ID.
pub struct MappackIdDisp<'a, 'b> {
    mappack_id: &'a AnyMappackId<'b>,
}

impl fmt::Display for MappackIdDisp<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.mappack_id {
            AnyMappackId::Event(_, edition) => {
                if let Some(id) = edition.mx_id {
                    fmt::Display::fmt(&id, f)
                } else {
                    write!(f, "__{}__{}__", edition.event_id, edition.id)
                }
            }
            AnyMappackId::Id(id) => f.write_str(id),
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

impl AnyMappackId<'_> {
    /// Returns a displayable version of the mappack ID.
    pub fn mappack_id(&self) -> MappackIdDisp<'_, '_> {
        MappackIdDisp { mappack_id: self }
    }

    /// Returns the related optional event.
    fn get_event(&self) -> OptEvent<'_, '_> {
        match self {
            Self::Event(event, edition) => OptEvent::new(event, edition),
            Self::Id(_) => Default::default(),
        }
    }

    /// Returns whether the mappack has a time-to-live or not.
    ///
    /// Only regular MX mappacks have a time-to-live.
    fn has_ttl(&self) -> bool {
        matches!(self, Self::Id(_))
    }

    /// Returns the optional time-to-live, in seconds, of the mappack.
    ///
    /// Only regular MX mappacks have a time-to-live.
    fn get_ttl(&self) -> Option<i64> {
        self.has_ttl().then_some(crate::env().mappack_ttl)
    }
}

/// Calculates the scores of the players on the provided mappack, and save the results
/// on the Redis database.
///
/// ## Parameters
///
/// * `mappack`: the mappack.
/// * `mysql_conn`: a connection to the MySQL/MariaDB database, to fetch the records.
/// * `redis_conn`: a connection to the Redis database, to store the scores.
#[cfg_attr(feature = "tracing", tracing::instrument(skip(db), err))]
pub async fn update_mappack(
    mappack: AnyMappackId<'_>,
    db: &mut DatabaseConnection,
) -> RecordsResult<usize> {
    // Calculate the scores
    let scores = calc_scores(mappack, db).await?;

    // Early return if the mappack has expired
    let Some(scores) = scores else {
        return Ok(0);
    };

    let total_scores = scores.scores.len();

    // Then save them to the Redis database for cache-handling
    save(mappack, scores, &mut db.redis_conn).await?;

    // And we save it to the registered mappacks set.
    if mappack.has_ttl() {
        // The mappack has a TTL, so its member will be removed from the set when
        // attempting to retrieve its maps.
        let _: () = db
            .redis_conn
            .sadd(mappacks_key(), mappack.mappack_id())
            .await?;
    }

    Ok(total_scores)
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip(scores, redis_conn)))]
async fn save(
    mappack: AnyMappackId<'_>,
    scores: MappackScores,
    redis_conn: &mut RedisConnection,
) -> RecordsResult<()> {
    // Here, we control all the Redis keys related to mappacks except `mappack_key`,
    // because this one is set only once.

    let set_options = SetOptions::default();
    let set_options = match mappack.get_ttl() {
        Some(ex) => {
            // Update expiration time of the mappack's maps set by the way
            let _: () = redis_conn.expire(mappack_key(mappack), ex).await?;

            set_options.with_expiration(SetExpiry::EX(ex as _))
        }
        None => {
            // Persist the mappack's maps set by the way
            let _: () = redis_conn.persist(mappack_key(mappack)).await?;

            set_options
        }
    };

    // --- Save the number of maps of the campaign

    let _: () = redis_conn
        .set_options(mappack_nb_map_key(mappack), scores.maps.len(), set_options)
        .await?;

    for map in &scores.maps {
        // --- Save the last rank on each map

        let _: () = redis_conn
            .set_options(
                mappack_map_last_rank(mappack, &map.map_id),
                map.last_rank,
                set_options,
            )
            .await?;

        if !mappack.has_ttl() {
            let _: () = redis_conn
                .persist(mappack_map_last_rank(mappack, &map.map_id))
                .await?;
        }
    }

    let _: () = redis_conn.del(mappack_lb_key(mappack)).await?;

    for score in scores.scores {
        let _: () = redis_conn
            .zadd(mappack_lb_key(mappack), score.player_id, score.rank)
            .await?;

        // --- Save the rank average

        let rank_avg = ((score.score + f64::EPSILON) * 100.).round() / 100.;

        let _: () = redis_conn
            .set_options(
                mappack_player_rank_avg_key(mappack, score.player_id),
                rank_avg,
                set_options,
            )
            .await?;

        // --- Save the amount of finished map

        let _: () = redis_conn
            .set_options(
                mappack_player_map_finished_key(mappack, score.player_id),
                score.maps_finished,
                set_options,
            )
            .await?;

        // --- Save their worst rank

        let _: () = redis_conn
            .set_options(
                mappack_player_worst_rank_key(mappack, score.player_id),
                score.worst.rank,
                set_options,
            )
            .await?;

        let _: () = redis_conn
            .del(mappack_player_ranks_key(mappack, score.player_id))
            .await?;

        for (game_id, rank) in score
            .ranks
            .into_iter()
            .map(|rank| (&scores.maps[rank.map_idx].map_id, rank.rank))
        {
            // --- Save their rank on each map

            let _: () = redis_conn
                .zadd(
                    mappack_player_ranks_key(mappack, score.player_id),
                    game_id,
                    rank,
                )
                .await?;
        }

        if let Some(ttl) = mappack.get_ttl() {
            let _: () = redis_conn
                .expire(mappack_player_ranks_key(mappack, score.player_id), ttl)
                .await?;
        } else {
            let _: () = redis_conn
                .persist(mappack_player_ranks_key(mappack, score.player_id))
                .await?;
            let _: () = redis_conn
                .persist(mappack_player_rank_avg_key(mappack, score.player_id))
                .await?;
            let _: () = redis_conn
                .persist(mappack_player_map_finished_key(mappack, score.player_id))
                .await?;
            let _: () = redis_conn
                .persist(mappack_player_worst_rank_key(mappack, score.player_id))
                .await?;
        }
    }

    // Set the time of the update
    if let Ok(time) = SystemTime::UNIX_EPOCH.elapsed() {
        let _: () = redis_conn
            .set(mappack_time_key(mappack), time.as_secs())
            .await?;
    }

    // Update the expiration time of the global keys
    if let Some(ttl) = mappack.get_ttl() {
        let _: () = redis_conn.expire(mappack_time_key(mappack), ttl).await?;
        let _: () = redis_conn.expire(mappack_lb_key(mappack), ttl).await?;
    } else {
        let _: () = redis_conn.persist(mappack_time_key(mappack)).await?;
        let _: () = redis_conn.persist(mappack_lb_key(mappack)).await?;
        let _: () = redis_conn.persist(mappack_nb_map_key(mappack)).await?;
    }

    Ok(())
}

/// Returns an `Option` because the mappack may have expired.
#[cfg_attr(feature = "tracing", tracing::instrument(skip(db)))]
async fn calc_scores(
    mappack: AnyMappackId<'_>,
    db: &mut DatabaseConnection,
) -> RecordsResult<Option<MappackScores>> {
    let mappack_key = mappack_key(mappack);
    let mappack_uids: Vec<String> = db.redis_conn.smembers(&mappack_key).await?;

    let mut maps = Vec::with_capacity(mappack_uids.len().max(5));

    let event = mappack.get_event();
    let (view_name, and_event) = event.get_view();

    let mappack = if mappack_uids.is_empty() {
        // If the mappack is empty, it means either that it's an invalid/unknown mappack ID,
        // or that its TTL has expired. So we remove its entry in the registered mappacks set.
        // The other keys related to this mappack were set with a TTL so they should
        // be deleted too.
        let _: i32 = db
            .redis_conn
            .srem(mappacks_key(), mappack.mappack_id())
            .await?;
        return Ok(None);
    } else {
        let mut out = Vec::with_capacity(mappack_uids.len());
        for map_uid in &mappack_uids {
            let map = must::have_map(&mut db.mysql_conn, map_uid).await?;
            maps.push(MappackMap {
                map_id: map.game_id.clone(),
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
            FROM {view_name} r
            INNER JOIN players p ON p.id = r.record_player_id
            WHERE map_id = ?
            {and_event}
            ORDER BY time ASC",
        );

        let query = sqlx::query_as::<_, RecordRow>(&query).bind(map.id);
        let query = if let Some((event, edition)) = event.0 {
            query.bind(event.id).bind(edition.id)
        } else {
            query
        };

        let res = query.fetch_all(&mut *db.mysql_conn).await?;

        let mut records = Vec::with_capacity(res.len());

        for record in res {
            if !scores.iter().any(|p| p.player_id == record.player_id2) {
                scores.push(PlayerScore {
                    player_id: record.player_id2,
                    ranks: Vec::new(),
                    score: 0.,
                    maps_finished: 0,
                    rank: 0,
                    worst: Default::default(),
                });
            }

            let record = RankedRecordRow {
                rank: get_rank(
                    db,
                    map.id,
                    record.record.record_player_id,
                    record.record.time,
                    event,
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
