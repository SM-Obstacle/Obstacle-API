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
use serde::{Deserialize, Serialize};
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
    db: &Database,
    login: &str,
    name: Option<&str>,
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
    db: &Database,
    game_id: &str,
    name: Option<&str>,
    author_login: Option<&str>,
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
    db: &Database,
    map_id: u32,
    player_id: u32,
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
    db: &Database,
    key: &str,
    map_id: u32,
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

#[derive(Deserialize, Serialize)]
#[serde(rename(serialize = "response"))]
pub struct HasFinishedResponse {
    #[serde(rename = "newBest")]
    pub has_improved: bool,
    pub login: String,
    pub old: i32,
    pub new: i32,
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
    db: &Database,
    login: String,
    map_game_id: String,
    map_id: u32,
    player_id: u32,
    time: i32,
    respawn_count: i32,
    flags: u32,
    inputs_path: String,
) -> Result<HasFinishedResponse, RecordsError> {
    let mut redis_conn = db.redis_pool.get().await.unwrap();

    let old_record = sqlx::query_as!(
        Record,
        "SELECT * FROM records WHERE map_id = ? AND player_id = ? ORDER BY record_date DESC LIMIT 1",
        map_id,
        player_id
    )
    .fetch_optional(&db.mysql_pool)
    .await?;

    let now = Utc::now().naive_utc();

    let (old, new, has_improved) = if let Some(Record { time: old, .. }) = old_record {
        if time < old {
            let inputs_expiry = None::<u32>;

            // Update redis record
            let key = format!("l0:{}", map_game_id);
            let _added: i64 = redis_conn.zadd(&key, player_id, time).await.unwrap_or(0);
            let _count = update_redis_leaderboard(db, &key, map_id).await?;

            sqlx::query!("INSERT INTO records (player_id, map_id, time, respawn_count, record_date, flags, inputs_path, inputs_expiry) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                player_id,
                map_id,
                time,
                respawn_count,
                now,
                flags,
                inputs_path,
                inputs_expiry
            )
                .execute(&db.mysql_pool)
                .await?;
        }
        (old, time, time < old)
    } else {
        let inputs_expiry = None::<u32>;

        sqlx::query!(
            "INSERT INTO records (player_id, map_id, time, respawn_count, record_date, flags, inputs_path, inputs_expiry) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            player_id,
            map_id,
            time,
            respawn_count,
            now,
            flags,
            inputs_path,
            inputs_expiry,
        )
            .execute(&db.mysql_pool)
            .await?;

        (time, time, true)
    };

    Ok(HasFinishedResponse {
        has_improved,
        login,
        old,
        new,
    })
}

pub async fn get_player_from_login(
    db: &Database,
    player_login: &str,
) -> Result<Option<u32>, RecordsError> {
    let r = sqlx::query_scalar!("SELECT id FROM players WHERE login = ?", player_login)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

pub async fn check_banned(
    db: &Database,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsError> {
    // TODO: set a trigger on insert on banishments table to check that a player is banned at most once at a time
    let r = sqlx::query_as::<_, Banishment>(
        "SELECT * FROM banishments WHERE player_id = ? AND (SYSDATE() < date_ban + duration OR duration = -1)"
    )
    .bind(player_id)
    .fetch_optional(&db.mysql_pool).await?;
    Ok(r)
}

pub async fn get_map_from_game_id(
    db: &actix_web::web::Data<Database>,
    map_game_id: &str,
) -> Result<Option<u32>, RecordsError> {
    let r = sqlx::query_scalar!("SELECT id FROM maps WHERE game_id = ?", map_game_id)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}
