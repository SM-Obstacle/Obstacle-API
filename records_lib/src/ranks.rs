//! Module which contains utility functions used to update maps leaderboards and get players ranks.
//!
//! When retrieving the rank of a player on a map, or updating its leaderboard, this module uses
//! a lock system.
//!
//! The Obstacle API uses the standard competition ranking system (1224) to order the leaderboards
//! of the maps. However, the server we use to store the maps leaderboards, Redis, doesn't support
//! the standard competition ranking system, but only the ordinal ranking (1234).
//!
//! Giving a map and a player we want to get the rank of, the alternative is to retrieve the rank
//! of the first player that has the same time. This requires executing many commands to
//! the Redis server:
//!
//! 1. `ZRANGEBYSCORE` to get the sorted list of the players having the same time
//! 2. `ZRANK` to get the rank of the first player of this list
//!
//! However, because Redis doesn't support transactional mode[^note], there are cases where
//! the leaderboard of a map is being both read and updated at the same time by many different API
//! operations. For example, when a player finishes a map, the leaderboard is updated ; at the same
//! time, another request may be reading the same leaderboard.
//!
//! Imagine a request A that needs to get the rank of a player on a map, and a request B that updates
//! the same leaderboard. Both requests run at the same time, so the time of the player in Redis on
//! this map can be updated *after* the request A started, but *before* the latter retrieves
//! his rank. Thus, the request A will retrieve the rank of a time that isn't in the Redis
//! leaderboard, because it has been previously updated to a newer time.
//!
//! The solution of replacing the player's time in the leaderboard by the time we're getting
//! the rank of, doesn't work. The leaderboard might be updated once again right after by
//! the request B[^note2].
//!
//! The only solution is to make a leaderboard lock system. If you need to read the leaderboard of
//! a map, you have to lock it, to avoid stale read issues ; and To lock a leaderboard, you need
//! to wait for any other pending lock.
//!
//! You may think that these are very rare cases, but they actually happen quite often. Especially
//! with the staggered requests system, and finish requests that can be numerous if the player
//! had a lot of cached requests. This causes a lot of updates of the same map leaderboard.
//! Many finish requests can also be sent during a cup, for example LoL cups, Choco cups, Campaigns,
//! etc.
//!
//! Any operation on a map's leaderboard must be done using functions defined in the [`ranks`](super)
//! module. This minimizes inconsistency issues by locking the leaderboards for each operation.
//!
//! However, we might still do reads/updates to the leaderboards without passing by this module, and
//! these can be done when a leaderboard is actually locked. We consider this case very rare.
//! Otherwise, to fix it, we would have to emulate the isolation part of a transactional mode in
//! any DBMS, so that all the operations on a leaderboard to get the rank of a player are made
//! on the same version of it ; and to emulate this, we have to clone the leaderboard
//! for each request, which is very cumbersome and slow. Thus, we ignore this issue.
//!
//! [^note]: Redis actually supports transactional mode, but the way it works is by queuing the
//! commands ; so we can't use it because we want to use the result of the previous commands before
//! running the next ones.
//! [^note2]: We update the player's time in the leaderboard anyway if the one we're getting the
//! rank of is lower than the one stored in the leaderboard, to keep the Redis leaderboard updated,
//! and minimize inconsistencies between Redis and MariaDB.

use crate::{
    context::{HasMapId, HasPlayerId},
    error::{RecordsError, RecordsResult},
    redis_key::{map_key, MapKey},
    DatabaseConnection, RedisConnection,
};
use deadpool_redis::redis::{self, AsyncCommands};
use futures::TryStreamExt;
use itertools::{EitherOrBoth, Itertools};

mod lock {
    use std::collections::HashSet;
    use std::future::Future;
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};
    use tokio::sync::RwLock;

    const WAIT_UNLOCK_TIMEOUT: Duration = Duration::from_secs(10);

    static LB_LOCK: LazyLock<RwLock<HashSet<u32>>> = LazyLock::new(Default::default);

    pub(super) async fn within<F, Fut, R>(map_id: u32, f: F) -> R
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = R>,
    {
        wait_unlock(map_id).await;
        LB_LOCK.write().await.insert(map_id);
        let r = f().await;
        LB_LOCK.write().await.remove(&map_id);
        r
    }

    async fn wait_unlock(map_id: u32) {
        let i = Instant::now();
        while i.elapsed() < WAIT_UNLOCK_TIMEOUT {
            if !LB_LOCK.read().await.contains(&map_id) {
                return;
            }
            tokio::task::yield_now().await;
        }
        #[cfg(feature = "tracing")]
        tracing::warn!(
            "Waiting for unlock of {map_id} exceeded {WAIT_UNLOCK_TIMEOUT:?}, unlocking it"
        );
        LB_LOCK.write().await.remove(&map_id);
        #[cfg(feature = "tracing")]
        tracing::debug!("Unlocked {map_id}");
    }
}

/// Updates the rank of a player on a map.
///
/// This is roughly just a `ZADD` command for the Redis leaderboard of the map.
/// The difference is that it locks the leaderboard during the operation so that any other request
/// that might access the ranks of the leaderboard must wait for it to finish.
///
/// # Arguments
///
/// * `redis_conn`: a connection to the Redis server
/// * `map_id`: The ID of the map
/// * `player_id`: The ID of the player
/// * `time`: The time of the player on this map
/// * `event`: The event context if provided
pub async fn update_rank<C>(conn: &mut RedisConnection, ctx: C, time: i32) -> RecordsResult<()>
where
    C: HasMapId + HasPlayerId,
{
    let _: () = lock::within(ctx.get_map_id(), || async move {
        conn.zadd(
            map_key(ctx.get_map_id(), ctx.get_opt_event_edition()),
            ctx.get_player_id(),
            time,
        )
        .await
    })
    .await?;

    Ok(())
}

async fn count_records_map<C: HasMapId>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<i64> {
    let builder = ctx.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM (SELECT r.* FROM records r ");
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" ");
    let query = builder
        .push_event_filter(&mut query, "eer")
        .push(" group by record_player_id) r")
        .build_query_scalar();

    query.fetch_one(conn).await.map_err(Into::into)
}

/// Checks if the Redis leaderboard for the map with the provided ID has a different count
/// than in the database, and reupdates the Redis leaderboard completly if so.
///
/// This is a check to avoid differences between the MariaDB and the Redis leaderboards.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> RecordsResult<i64> {
    let mysql_count: i64 = count_records_map(conn.mysql_conn, &ctx).await?;

    lock::within(ctx.get_map_id(), || async move {
        let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition());

        let redis_count: i64 = conn.redis_conn.zcount(key, "-inf", "+inf").await?;
        if redis_count != mysql_count {
            force_update(conn, ctx).await?;
        }
        RecordsResult::Ok(())
    })
    .await?;

    Ok(mysql_count)
}

fn get_mariadb_lb_query<C: HasMapId>(ctx: C) -> sqlx::QueryBuilder<'static, sqlx::MySql> {
    let builder = ctx.sql_frag_builder();

    let mut q = sqlx::QueryBuilder::new(
        "select record_player_id, min(time) as time \
        from records r ",
    );

    builder
        .push_event_join(&mut q, "eer", "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" ");
    builder
        .push_event_filter(&mut q, "eer")
        .push("group by record_player_id order by time, record_player_id asc");

    q
}

async fn force_update_locked<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> RecordsResult<()> {
    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition()).to_string();

    pipe.del(&key);

    get_mariadb_lb_query(&ctx)
        .build_query_as()
        .fetch(&mut **conn.mysql_conn)
        .map_ok(|(player_id, time): (u32, i32)| {
            pipe.zadd(&key, player_id, time);
        })
        .try_collect::<()>()
        .await?;

    let _: () = pipe.query_async(conn.redis_conn).await?;

    Ok(())
}

/// Updates the Redis leaderboard for a map.
///
/// This function deletes the Redis key and reinserts all the records from the MariaDB database.
/// All this is done in a single transaction.
pub async fn force_update<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> RecordsResult<()> {
    lock::within(ctx.get_map_id(), || async move {
        force_update_locked(conn, ctx).await
    })
    .await
}

async fn get_rank_impl(
    redis_conn: &mut RedisConnection,
    key: &MapKey<'_>,
    time: i32,
) -> RecordsResult<Option<i32>> {
    let player_ids: Vec<u32> = redis_conn
        .zrangebyscore_limit(key, time, time, 0, 1)
        .await?;

    match player_ids.first() {
        Some(id) => {
            let rank: Option<i32> = redis_conn.zrank(key, *id).await?;
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
///
/// See the [module documentation](super) for more information.
pub async fn get_rank<C>(db: &mut DatabaseConnection<'_>, ctx: C, time: i32) -> RecordsResult<i32>
where
    C: HasMapId + HasPlayerId,
{
    lock::within(ctx.get_map_id(), || async move {
        let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition());

        // We update the Redis leaderboard if it doesn't have the requested `time`.
        let score: Option<i32> = db.redis_conn.zscore(&key, ctx.get_player_id()).await?;
        let newest_time = match score {
            Some(t) if t == time => None,
            Some(t) if t < time => {
                let _: () = db.redis_conn.zadd(&key, ctx.get_player_id(), time).await?;
                Some(t)
            }
            _ => {
                let _: () = db.redis_conn.zadd(&key, ctx.get_player_id(), time).await?;
                None
            }
        };

        match get_rank_impl(db.redis_conn, &key, time).await? {
            Some(r) => {
                if let Some(time) = newest_time {
                    let _: () = db.redis_conn.zadd(key, ctx.get_player_id(), time).await?;
                }
                Ok(r)
            }
            None => Err(get_rank_failed(db, ctx, time, score).await?),
        }
    })
    .await
}

/// Panics with a clear message of the leaderboards differences between MariaDB and Redis.
///
/// The `O` generic parameter is used to return the same type as the [`get_rank`] function.
#[cold]
async fn get_rank_failed<C>(
    db: &mut DatabaseConnection<'_>,
    ctx: C,
    time: i32,
    tested_time: Option<i32>,
) -> RecordsResult<RecordsError>
where
    C: HasPlayerId + HasMapId,
{
    use std::fmt::Write as _;

    fn num_digits<N>(n: N) -> usize
    where
        f64: From<N>,
    {
        (f64::from(n).log10() + 1.) as _
    }

    let event = ctx.get_opt_event_edition();
    let player_id = ctx.get_player_id();
    let map_id = ctx.get_map_id();

    let key = &map_key(ctx.get_map_id(), event);
    let redis_lb: Vec<i64> = db.redis_conn.zrange_withscores(key, 0, -1).await?;

    let mariadb_lb = get_mariadb_lb_query(&ctx)
        .build_query_as::<(u32, i32)>()
        .fetch_all(&mut **db.mysql_conn)
        .await?;

    let lb = redis_lb
        .chunks_exact(2)
        .map(|chunk| (chunk[0] as u32, chunk[1] as i32))
        .zip_longest(mariadb_lb)
        .collect::<Vec<_>>();

    let width = lb
        .iter()
        .map(|e| match e {
            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                num_digits(*rpid) + num_digits(*rtime) + num_digits(*mpid) + num_digits(*mtime)
            }
            EitherOrBoth::Left((rpid, rtime)) => num_digits(*rpid) + num_digits(*rtime),
            EitherOrBoth::Right((mpid, mtime)) => num_digits(*mpid) + num_digits(*mtime),
        })
        .max()
        .unwrap_or_default()
        .max("player".len() * 2 + "time".len() * 2 + 1);
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

    for row in lb {
        match row {
            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                writeln!(
                    msg,
                    "{c}{rpid:w4$} | {rtime:w4$} || {mpid:w4$} | {mtime:w4$}{c_end}",
                    c = if rpid != mpid || rtime != mtime {
                        "\x1b[93m"
                    } else if player_id == rpid && player_id == mpid {
                        "\x1b[34m"
                    } else {
                        ""
                    },
                    c_end = if rpid != mpid
                        || rtime != mtime
                        || (player_id == rpid && player_id == mpid)
                    {
                        "\x1b[0m"
                    } else {
                        ""
                    },
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

    #[cfg(feature = "tracing")]
    tracing::error!("missing player rank ({player_id} on map {map_id} with time {time}); tested time: {tested_time:?}\n{msg}");

    Ok(RecordsError::Internal)
}
