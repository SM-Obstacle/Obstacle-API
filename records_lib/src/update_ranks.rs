//! This is a tiny module which contains utility functions used to update the maps leaderboards
//! in Redis.
//!
//! See the [`update_leaderboard`] and [`get_rank`] functions for more information.

use deadpool_redis::redis::{self, AsyncCommands};
use futures::{Stream, TryStreamExt};
use itertools::{EitherOrBoth, Itertools};
use sqlx::{pool::PoolConnection, MySql, MySqlConnection};

use crate::{
    error::RecordsResult,
    event::OptEvent,
    redis_key::{map_key, MapKey},
    DatabaseConnection, RedisConnection,
};

async fn count_records_map(
    db: &mut MySqlConnection,
    map_id: u32,
    event: OptEvent<'_, '_>,
) -> RecordsResult<i64> {
    let (join_event, and_event) = event.get_join();

    let q = &format!(
        "SELECT COUNT(*)
        FROM (SELECT r.* FROM records r {join_event}
        WHERE map_id = ? {and_event}
        GROUP BY record_player_id) r",
    );

    let q = sqlx::query_scalar(q).bind(map_id);

    let q = if let Some((ev, ed)) = event.0 {
        q.bind(ev.id).bind(ed.id)
    } else {
        q
    };

    q.fetch_one(db).await.map_err(Into::into)
}

/// Checks if the Redis leaderboard for the map with the `key` has a different count
/// that in the database, and reupdates the Redis leaderboard completly if so.
///
/// This is a check to avoid records duplicates, which may happen sometimes.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard(
    db: &mut DatabaseConnection,
    map_id: u32,
    event: OptEvent<'_, '_>,
) -> RecordsResult<i64> {
    let key = map_key(map_id, event);
    let redis_count: i64 = db.redis_conn.zcount(&key, "-inf", "+inf").await?;
    let mysql_count: i64 = count_records_map(&mut db.mysql_conn, map_id, event).await?;

    if redis_count != mysql_count {
        force_update(map_id, event, db, &key).await?;
    }

    Ok(mysql_count)
}

fn get_mariadb_lb_query(event: OptEvent<'_, '_>) -> String {
    let (join_event, and_event) = event.get_join();

    format!(
        "SELECT record_player_id, min(time) AS time
            FROM records r
            {join_event}
                WHERE map_id = ?
                    {and_event}
                GROUP BY record_player_id
                ORDER BY time, record_date ASC",
    )
}

fn get_mariadb_lb<'a>(
    db: &'a mut PoolConnection<MySql>,
    event: OptEvent<'_, '_>,
    map_id: u32,
    query: &'a str,
) -> impl Stream<Item = sqlx::Result<(u32, i32)>> + 'a {
    let mut query = sqlx::query_as(query).bind(map_id);
    if let Some((event, edition)) = event.0 {
        query = query.bind(event.id).bind(edition.id);
    }

    query.fetch(&mut **db)
}

async fn force_update(
    map_id: u32,
    event: OptEvent<'_, '_>,
    db: &mut DatabaseConnection,
    key: &MapKey<'_>,
) -> Result<(), crate::error::RecordsError> {
    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    pipe.del(key);

    get_mariadb_lb(
        &mut db.mysql_conn,
        event,
        map_id,
        &get_mariadb_lb_query(event),
    )
    .map_ok(|(player_id, time): (u32, i32)| {
        pipe.zadd(key, player_id, time);
    })
    .try_collect::<()>()
    .await?;

    let _: () = pipe.query_async(&mut db.redis_conn).await?;
    Ok(())
}

/// Gets the rank of a player in a map, or None if not found.
///
/// The ranking type is the standard competition ranking (1224).
pub async fn get_rank_opt(
    redis_conn: &mut RedisConnection,
    key: &MapKey<'_>,
    player_id: u32,
) -> RecordsResult<Option<i32>> {
    // Get the score (time) of the player
    let Some(time): Option<i32> = redis_conn.zscore(key, player_id).await? else {
        return Ok(None);
    };

    let player_id: Vec<u32> = redis_conn
        .zrangebyscore_limit(key, time, time, 0, 1)
        .await?;

    match player_id.first() {
        Some(id) => {
            let rank: Option<i32> = redis_conn.zrank(key, id).await?;
            Ok(rank.map(|rank| rank + 1))
        }
        None => Ok(None),
    }
}

/// Gets the rank of a player in a map, or fully updates its leaderboard if not found.
///
/// The full update means a delete of the Redis key then a reinsertion of all the records.
/// This may be called when the SQL and Redis databases had the same amount of records on a map,
/// but the times were not corresponding. It generally happens after a database migration.
///
/// The ranking type is the standard competition ranking (1224).
pub async fn get_rank(
    db: &mut DatabaseConnection,
    map_id: u32,
    player_id: u32,
    event: OptEvent<'_, '_>,
) -> RecordsResult<i32> {
    let key = &map_key(map_id, event);

    match get_rank_opt(&mut db.redis_conn, key, player_id).await? {
        Some(rank) => Ok(rank),
        None => {
            let _: () = db.redis_conn.del(key).await?;
            force_update(map_id, event, db, key).await?;

            match get_rank_opt(&mut db.redis_conn, key, player_id).await? {
                Some(rank) => Ok(rank),
                None => {
                    let redis_lb: Vec<i64> = db.redis_conn.zrange_withscores(key, 0, -1).await?;
                    let mariadb_lb = get_mariadb_lb(
                        &mut db.mysql_conn,
                        event,
                        map_id,
                        &get_mariadb_lb_query(event),
                    )
                    .try_collect::<Vec<_>>()
                    .await?;

                    let lb = redis_lb
                        .chunks_exact(2)
                        .map(|chunk| (chunk[0] as u32, chunk[1] as i32))
                        .zip_longest(mariadb_lb)
                        .collect::<Vec<_>>();

                    fn num_digits<N>(n: N) -> usize
                    where
                        f64: From<N>,
                    {
                        (f64::from(n).log10() + 1.) as _
                    }

                    let width = lb
                        .iter()
                        .map(|e| match e {
                            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                                num_digits(*rpid)
                                    + num_digits(*rtime)
                                    + num_digits(*mpid)
                                    + num_digits(*mtime)
                            }
                            EitherOrBoth::Left((rpid, rtime)) => {
                                num_digits(*rpid) + num_digits(*rtime)
                            }
                            EitherOrBoth::Right((mpid, mtime)) => {
                                num_digits(*mpid) + num_digits(*mtime)
                            }
                        })
                        .max()
                        .unwrap_or_default()
                        .max(
                            "player".len() * 2 + "time".len() * 2 + 1
                        );

                    let w4 = width / 4;

                    let mut msg = format!(
                        "{:w2$} || {:w2$}\n{empty:-<w$}\n{player:w4$} | {time:w4$} || {player:w4$} | {time:w4$}\n",
                        "redis",
                        "mariadb",
                        player = "player",
                        time = "time",
                        w2 = width / 2 + 3,
                        w = width + 10,
                        empty = "",
                    );

                    use std::fmt::Write as _;

                    for row in lb {
                        match row {
                            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                                writeln!(
                                    msg,
                                    "{c}{rpid:w4$} | {rtime:w4$} || {mpid:w4$} | {mtime:w4$}{c_end}",
                                    c = if rpid != mpid || rtime != mtime { "\x1b[93m" } else { "" },
                                    c_end = if rpid != mpid || rtime != mtime { "\x1b[0m" } else { "" },
                                )
                                .unwrap();
                            }
                            EitherOrBoth::Left((rpid, rtime)) => {
                                writeln!(
                                    msg,
                                    "\x1b[93m{rpid:w4$} | {rtime:w4$} || {empty:w4$} | {empty:w4$}\x1b[0m",
                                    empty = ""
                                )
                                .unwrap();
                            }
                            EitherOrBoth::Right((mpid, mtime)) => {
                                writeln!(
                                    msg,
                                    "\x1b[93m{empty:w4$} | {empty:w4$} || {mpid:w4$} | {mtime:w4$}\x1b[0m",
                                    empty = ""
                                )
                                .unwrap();
                            }
                        }
                    }

                    let redis_lb2: Vec<i64> = db.redis_conn.zrange(key, 0, -1).await?;
                    let redis_lb2 = redis_lb2.len();

                    panic!("missing player rank ({player_id} on {map_id})\n2nd zrange count: {redis_lb2:?}\n{msg}");
                }
            }
        }
    }
}
