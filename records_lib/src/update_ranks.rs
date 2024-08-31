//! This is a tiny module which contains utility functions used to update the maps leaderboards
//! in Redis.
//!
//! See the [`update_leaderboard`] and [`get_rank`] functions for more information.

use deadpool_redis::redis::{self, AsyncCommands};
use sqlx::MySqlConnection;

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
        FROM (SELECT * FROM records r {join_event}
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

async fn force_update(
    map_id: u32,
    event: OptEvent<'_, '_>,
    db: &mut DatabaseConnection,
    key: &MapKey<'_>,
) -> Result<(), crate::error::RecordsError> {
    let (join_event, and_event) = event.get_join();

    let query = format!(
        "SELECT record_player_id, min(time) AS time
            FROM records r
            {join_event}
                WHERE map_id = ?
                    {and_event}
                GROUP BY record_player_id
                ORDER BY time, record_date ASC",
    );

    let mut query = sqlx::query_as(&query).bind(map_id);
    if let Some((event, edition)) = event.0 {
        query = query.bind(event.id).bind(edition.id);
    }

    let all_map_records: Vec<(u32, i32)> = query.fetch_all(&mut *db.mysql_conn).await?;
    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    pipe.del(key);

    for record in all_map_records {
        pipe.zadd(key, record.0, record.1);
    }

    pipe.query_async(&mut db.redis_conn).await?;
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
            db.redis_conn.del(key).await?;
            force_update(map_id, event, db, key).await?;

            loop {
                if let Some(rank) = get_rank_opt(&mut db.redis_conn, key, player_id).await? {
                    return Ok(rank);
                }
                tokio::task::yield_now().await;
            }
        }
    }
}
