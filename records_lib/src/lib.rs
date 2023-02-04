pub mod database;
pub mod error;
pub mod escape;
pub mod graphql;
pub mod models;

use crate::models::*;
use chrono::Utc;
pub use database::*;
use deadpool_redis::redis::AsyncCommands;
pub use error::RecordsError;
use sqlx::{mysql::MySqlRow, Row};

/// Update a player login and nickname, it will create the player if it doesn't exists and returns its id
///
/// # Arguments
///
/// * `db` - A data source
/// * `login` - The maniaplanet account login
/// * `name` - The name of the player
///
pub async fn update_player(
    db: &Database, login: &str, name: Option<&str>,
) -> Result<u32, RecordsError> {
    let name = name.unwrap_or(login);

    let player_id = sqlx::query_scalar!("SELECT id from players where login = ?", login)
        .fetch_optional(&db.mysql_pool)
        .await?;

    if let Some(player_id) = player_id {
        sqlx::query!("UPDATE players SET name = ? WHERE id = ?", name, player_id)
            .execute(&db.mysql_pool)
            .await?;

        Ok(player_id)
    } else {
        sqlx::query!(
            "INSERT INTO players (login, name) VALUES (?, ?) RETURNING id",
            login,
            name
        )
        .map(|row: MySqlRow| -> Result<u32, sqlx::Error> { row.try_get(0) })
        .fetch_one(&db.mysql_pool)
        .await?
        .map_err(|e| e.into())
    }
}

/// Returns the id of a player, creates it if it doesn't exist
///
/// # Arguments
///
/// * `db` - A data source
/// * `login` - The maniaplanet account login
///
pub async fn select_or_insert_player(db: &Database, login: &str) -> Result<u32, RecordsError> {
    let query = sqlx::query_scalar!("SELECT id from players where login = ?", login);

    if let Some(id) = query.fetch_optional(&db.mysql_pool).await? {
        Ok(id)
    } else {
        sqlx::query!(
            "INSERT INTO players (login, name) VALUES (?, ?) RETURNING id",
            login,
            login
        )
        .map(|row: MySqlRow| -> Result<u32, sqlx::Error> { row.try_get(0) })
        .fetch_one(&db.mysql_pool)
        .await?
        .map_err(|e| e.into())
    }
}

/// Fetch a player from the database
///
/// # Arguments
///
/// * `db` - A data source
/// * `id` - The player id
///
pub async fn select_player(db: &Database, id: u32) -> Result<Player, RecordsError> {
    sqlx::query_as!(Player, "SELECT * from players WHERE id = ?", id)
        .fetch_one(&db.mysql_pool)
        .await
        .map_err(|e| e.into())
}

/// Update a map game_id, name and author, it will insert the map if needed and returns the id of the map
///
/// # Arguments
///
/// * `db` - A data source
/// * `game_id` - The maniaplanet UUID of the map
/// * `name` - The name of the map
/// * `author_login` - The login of the map creator
///
pub async fn update_map(
    db: &Database, game_id: &str, name: Option<&str>, author_login: Option<&str>,
) -> Result<u32, RecordsError> {
    let name = name.unwrap_or("Unknown map");

    let author_login = author_login.unwrap_or("smokegun");

    let player_id = select_or_insert_player(&db, &author_login).await?;

    let map_id = sqlx::query_scalar!("SELECT id from maps where game_id = ?", game_id)
        .fetch_optional(&db.mysql_pool)
        .await?;

    if let Some(map_id) = map_id {
        sqlx::query!(
            "UPDATE maps SET name = ?, player_id = ? WHERE id = ?",
            name,
            player_id,
            map_id
        )
        .execute(&db.mysql_pool)
        .await?;

        Ok(map_id)
    } else {
        sqlx::query!(
            "INSERT INTO maps (game_id, player_id, name) VALUES (?, ?, ?) RETURNING id",
            game_id,
            player_id,
            name
        )
        .map(|row: MySqlRow| -> Result<u32, sqlx::Error> { row.try_get(0) })
        .fetch_one(&db.mysql_pool)
        .await?
        .map_err(|e| e.into())
    }
}

/// Returns the id of a map, creates it if it doesn't exist
///
/// # Arguments
///
/// * `db` - A data source
/// * `login` - The maniaplanet account login
///
pub async fn select_or_insert_map(db: &Database, game_id: &str) -> Result<u32, RecordsError> {
    let query = sqlx::query_scalar!("SELECT id from maps where game_id = ?", game_id);

    if let Some(id) = query.fetch_optional(&db.mysql_pool).await? {
        Ok(id)
    } else {
        sqlx::query!(
            "INSERT INTO maps (game_id, player_id, name) VALUES (?, ?, ?) RETURNING id",
            game_id,
            "smokegun",
            "Unknown map"
        )
        .map(|row: MySqlRow| -> Result<u32, sqlx::Error> { row.try_get(0) })
        .fetch_one(&db.mysql_pool)
        .await?
        .map_err(|e| e.into())
    }
}

/// Fetch a map from the database
///
/// # Arguments
///
/// * `db` - A data source
/// * `id` - The map id
///
pub async fn select_map(db: &Database, id: u32) -> Result<Map, RecordsError> {
    sqlx::query_as!(Map, "SELECT * from maps WHERE id = ?", id)
        .fetch_one(&db.mysql_pool)
        .await
        .map_err(|e| e.into())
}

/// Returns the number of records in a map
///
/// # Arguments
///
/// * `db` - A data source
/// * `game_id` - The maniaplanet UUID of the map
///
pub async fn count_records_map(db: &Database, map_id: u32) -> Result<i64, RecordsError> {
    sqlx::query_scalar!("SELECT count(*) FROM records WHERE map_id = ?", map_id)
        .fetch_one(&db.mysql_pool)
        .await
        .map_err(|e| e.into())
}

/// Fetch a record from the database
///
/// # Arguments
///
/// * `db` - A data source
/// * `map_id` - The map id
/// * `player_id` - The player id
///
pub async fn select_record(
    db: &Database, map_id: u32, player_id: u32,
) -> Result<Record, RecordsError> {
    sqlx::query_as!(
        Record,
        "SELECT * from records WHERE map_id = ? AND player_id = ?",
        map_id,
        player_id
    )
    .fetch_one(&db.mysql_pool)
    .await
    .map_err(|e| e.into())
}

pub async fn update_redis_leaderboard(
    db: &Database, key: &str, map_id: u32,
) -> Result<i64, RecordsError> {
    let mut redis_conn = db.redis_pool.get().await.unwrap();
    let redis_count: i64 = redis_conn.zcount(key, "-inf", "+inf").await.unwrap();
    let mysql_count: i64 = count_records_map(db, map_id).await?;
    if redis_count != mysql_count {
        let all_map_records =
            sqlx::query_as!(Record, "SELECT * FROM records where map_id = ?", map_id)
                .fetch_all(&db.mysql_pool)
                .await?;

        let _removed_count: i64 = redis_conn.del(key).await.unwrap_or(0);

        for record in all_map_records {
            let _: i64 = redis_conn
                .zadd(key, record.player_id, record.time)
                .await
                .unwrap_or(0);
        }
    }

    Ok(mysql_count)
}

/// Update a record to the database
///
/// # Arguments
///
/// * `db` - A data source
/// * `map_id` - The map id
/// * `player_id` - The player id
/// * `time` - The new record's time
/// * `respawn_count` - The new record's respawn count
///
pub async fn player_new_record(
    db: &Database, map_game_id: &str, map_id: u32, player_id: u32, time: i32, respawn_count: i32,
    flags: u32,
) -> Result<(Option<Record>, Record), RecordsError> {
    let mut redis_conn = db.redis_pool.get().await.unwrap();

    let old_record = sqlx::query_as!(
        Record,
        "SELECT * FROM records WHERE map_id = ? AND player_id = ?",
        map_id,
        player_id
    )
    .fetch_optional(&db.mysql_pool)
    .await?;

    let now = Utc::now().naive_utc();

    if let Some(old_record) = old_record {
        let mut new_record = old_record.clone();
        new_record.time = time;
        new_record.respawn_count = respawn_count;
        new_record.try_count += 1;
        new_record.updated_at = now;
        new_record.flags = flags;

        if time < old_record.time {
            // Update redis record
            let key = format!("l0:{}", map_game_id);
            let _added: i64 = redis_conn.zadd(&key, player_id, time).await.unwrap_or(0);
            let _count = update_redis_leaderboard(db, &key, map_id).await?;

            sqlx::query!("UPDATE records SET time = ?, respawn_count = ?, try_count = ?, updated_at = ?, flags = ? WHERE id = ?",
                        new_record.time,
                        new_record.respawn_count,
                        new_record.try_count,
                        new_record.updated_at,
                        new_record.flags,
                        new_record.id)
                .execute(&db.mysql_pool)
                .await?;
        }

        Ok((Some(old_record), new_record))
    } else {
        sqlx::query!(
            "INSERT INTO records (player_id, map_id, time, respawn_count, try_count, created_at, updated_at, flags) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id, player_id, map_id, time, respawn_count, try_count, created_at, updated_at, flags",
            player_id,
            map_id,
            time,
            respawn_count,
            1,
            now,
            now,
            flags
        )
            .map(|row: MySqlRow|  { Ok((None, Record {
                id: row.try_get(0).unwrap(),
                player_id: row.try_get(1).unwrap(),
                map_id: row.try_get(2).unwrap(),
                time: row.try_get(3).unwrap(),
                respawn_count: row.try_get(4).unwrap(),
                try_count: row.try_get(5).unwrap(),
                created_at: row.try_get(6).unwrap(),
                updated_at: row.try_get(7).unwrap(),
                flags: row.try_get(8).unwrap(),
            })) })
        .fetch_one(&db.mysql_pool)
        .await?
    }
}
