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
    RedisConnection,
    error::{RecordsError, RecordsResult},
    opt_event::OptEvent,
    redis_key::{MapKey, map_key},
};
use deadpool_redis::redis::{self, AsyncCommands};
use entity::{event_edition_records, records};
use futures::TryStreamExt;
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, Order, PaginatorTrait, QueryFilter as _,
    QueryOrder as _, QuerySelect, QueryTrait as _, SelectModel, Selector, StreamTrait,
    sea_query::expr,
};

use std::{
    collections::HashMap,
    fmt,
    future::Future,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};

struct RankComputeErrorInner {
    player_id: u32,
    map_id: u32,
    event: Option<(u32, u32)>,
    raw_redis_lb: Vec<i64>,
    sql_lb: Vec<DbLeaderboardItem>,
    time: i32,
    tested_time: Option<i32>,
}

impl fmt::Debug for RankComputeErrorInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RankComputeErrorInner")
            .field("player_id", &self.player_id)
            .field("map_id", &self.map_id)
            .field("event", &self.event)
            .field("time", &self.time)
            .field("tested_time", &self.tested_time)
            .finish_non_exhaustive()
    }
}

/// Error returned when failing to compute a rank.
#[derive(Debug, Clone, thiserror::Error)]
#[error("rank compute error")]
pub struct RankComputeError {
    inner: Arc<RankComputeErrorInner>,
}

impl RankComputeError {
    /// The ID of the player whose rank was attempted to be computed.
    pub fn player_id(&self) -> u32 {
        self.inner.player_id
    }

    /// The ID of the map on which the rank of the player was attempted to be computed.
    pub fn map_id(&self) -> u32 {
        self.inner.map_id
    }

    /// The current event when computing the rank.
    pub fn event(&self) -> Option<(u32, u32)> {
        self.inner.event
    }

    /// The raw content of the Redis leaderboard, i.e. computed with ZRANGEWITHSCORES.
    pub fn raw_redis_lb(&self) -> &[i64] {
        &self.inner.raw_redis_lb
    }

    /// The leaderboard of the SQL database.
    pub fn sql_lb(&self) -> &[DbLeaderboardItem] {
        &self.inner.sql_lb
    }

    /// The time of the player which was used to calculate the score.
    pub fn time(&self) -> i32 {
        self.inner.time
    }

    /// The initial time of the player on the Redis leaderboard, before updating it.
    pub fn tested_time(&self) -> Option<i32> {
        self.inner.tested_time
    }
}

/// Wraps the execution of the provided closure to guarantee that it is executed by one task
/// at a time, based on the provided ID.
///
/// This is mainly used to avoid stale-read issues when processing leaderboards in Redis.
///
/// If many tasks wrap their procedure for the same ID, it is guaranteed that only one of them
/// will execute at a time, implying the other tasks to wait before executing the next one.
///
/// Therefore, this function may await for up to 10 seconds.
pub async fn lock_within<F, Fut, R>(map_id: u32, f: F) -> R
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = R>,
{
    const WAIT_UNLOCK_TIMEOUT: Duration = Duration::from_secs(10);

    /// Contains the locked IDs.
    ///
    /// Any entry in the hash map doesn't guarantee that the ID in question is actually locked.
    /// It is locked if and only if the associated semaphore is acquired. This is so that
    /// we don't remove the entry from the hash map then reinsert it again if another task is running
    /// for the same ID. The entry is removed if the associated semaphore is referenced only once.
    static LOCKS: LazyLock<Mutex<HashMap<u32, Arc<Semaphore>>>> = LazyLock::new(Default::default);

    let semaphore = {
        let mut locks = LOCKS.lock().await;
        match locks.get(&map_id).cloned() {
            Some(semaphore) => semaphore,
            None => {
                let semaphore = Arc::new(Semaphore::new(1));
                locks.insert(map_id, semaphore.clone());
                semaphore
            }
        }
    };

    let ret = {
        let _permit =
            match tokio::time::timeout(WAIT_UNLOCK_TIMEOUT, semaphore.acquire_owned()).await {
                Ok(permit) => Some(permit),
                Err(_) => {
                    // Remove the entry if we timeout
                    LOCKS.lock().await.remove(&map_id);
                    None
                }
            };

        f().await
    };

    let mut locks = LOCKS.lock().await;
    if let Some(1) = locks.get(&map_id).map(Arc::strong_count) {
        // Remove the entry if we're the last
        locks.remove(&map_id);
    }

    ret
}

/// Updates the rank of a player on a map.
///
/// This is roughly just a `ZADD` command for the Redis leaderboard of the map.
/// The difference is that it locks the leaderboard during the operation, to avoid modifying
/// the leaderboard while another request handler operates on it.
pub async fn update_rank(
    conn: &mut RedisConnection,
    map_id: u32,
    player_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    let _: () = lock_within(map_id, || {
        conn.zadd(map_key(map_id, event), player_id, time)
    })
    .await?;

    Ok(())
}

async fn count_records_map<C: ConnectionTrait>(
    conn: &C,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let query = records::Entity::find()
        .filter(records::Column::MapId.eq(map_id))
        .group_by(records::Column::RecordPlayerId);

    let query = match event.get() {
        Some((ev, ed)) => query.reverse_join(event_edition_records::Entity).filter(
            event_edition_records::Column::EventId
                .eq(ev.id)
                .and(event_edition_records::Column::EditionId.eq(ed.id)),
        ),
        None => query,
    };

    let result = query.count(conn).await?;
    Ok(result)
}

/// Checks if the Redis leaderboard for the map with the provided ID has a different count
/// than in the database, and regenerates the Redis leaderboard completely if so.
///
/// This is a check to avoid differences between the MariaDB and the Redis leaderboards.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let mysql_count = count_records_map(conn, map_id, event).await?;

    lock_within(map_id, || async move {
        let key = map_key(map_id, event);

        let redis_count: u64 = redis_conn.zcount(key, "-inf", "+inf").await?;
        if redis_count != mysql_count {
            force_update_locked(conn, redis_conn, map_id, event).await?;
        }

        RecordsResult::Ok(())
    })
    .await?;

    Ok(mysql_count)
}

/// A leaderboard row, used in [`RankComputeError`].
#[derive(Debug, sea_orm::FromQueryResult)]
pub struct DbLeaderboardItem {
    /// The ID of the player who made the record.
    pub record_player_id: u32,
    /// The record time.
    pub time: i32,
}

fn get_mariadb_lb_query(
    map_id: u32,
    event: OptEvent<'_>,
) -> Selector<SelectModel<DbLeaderboardItem>> {
    records::Entity::find()
        .filter(records::Column::MapId.eq(map_id))
        .group_by(records::Column::RecordPlayerId)
        .order_by(records::Column::Time.min(), Order::Asc)
        .apply_if(event.get(), |builder, (ev, ed)| {
            builder.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
        .select_only()
        .column(records::Column::RecordPlayerId)
        .column_as(expr::Expr::col(records::Column::Time).min(), "time")
        .into_model()
}

async fn force_update_locked<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    let key = map_key(map_id, event).to_string();

    pipe.del(&key);

    get_mariadb_lb_query(map_id, event)
        .stream(conn)
        .await?
        .map_ok(|item| {
            pipe.zadd(&key, item.record_player_id, item.time);
        })
        .try_collect::<()>()
        .await?;

    let _: () = pipe.query_async(redis_conn).await?;

    Ok(())
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
pub async fn get_rank<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    player_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let key = map_key(map_id, event);

    lock_within(map_id, || async move {
        // We update the Redis leaderboard if it doesn't have the requested `time`, and keep
        // track of the previous time if it's lower than ours.
        let score: Option<i32> = redis_conn.zscore(&key, player_id).await?;
        let newest_time = match score {
            Some(t) if t == time => None,
            other => {
                force_update_locked(conn, redis_conn, map_id, event).await?;
                other.filter(|t| *t < time)
            }
        };

        match get_rank_impl(redis_conn, &key, time).await? {
            Some(r) => {
                if let Some(time) = newest_time {
                    let _: () = redis_conn.zadd(key, player_id, time).await?;
                }
                Ok(r)
            }
            None => Err(
                get_rank_failed(conn, redis_conn, player_id, map_id, event, time, score).await?,
            ),
        }
    })
    .await
}

/// Returns an error and prints a clear message of the leaderboards differences between
/// MariaDB and Redis.
#[cold]
async fn get_rank_failed<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    player_id: u32,
    map_id: u32,
    event: OptEvent<'_>,
    time: i32,
    tested_time: Option<i32>,
) -> RecordsResult<RecordsError> {
    let key = &map_key(map_id, event);
    let redis_lb: Vec<i64> = redis_conn.zrange_withscores(key, 0, -1).await?;
    let mariadb_lb = get_mariadb_lb_query(map_id, event).all(conn).await?;

    Err(RecordsError::RankCompute(RankComputeError {
        inner: Arc::new(RankComputeErrorInner {
            player_id,
            map_id,
            event: event.get().map(|(ev, ed)| (ev.id, ed.id)),
            raw_redis_lb: redis_lb,
            sql_lb: mariadb_lb,
            time,
            tested_time,
        }),
    }))
}
