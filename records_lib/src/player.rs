//! This module contains anything related to in-game players in this library.

use crate::event::OptEventIds;
use crate::{error::RecordsResult, models::Player};
use sqlx::MySqlConnection;

/// Returns the time of a player on a map.
pub async fn get_time_on_map(
    conn: &mut MySqlConnection,
    player_id: u32,
    map_id: u32,
    event: OptEventIds,
) -> RecordsResult<Option<i32>> {
    let (view_name, and_event) = event.get_view();

    let query = format!(
        "select time from {view_name} r where map_id = ? and record_player_id = ? {and_event}"
    );
    let query = sqlx::query_scalar(&query).bind(map_id).bind(player_id);
    let query = if let Some((ev, ed)) = event.0 {
        query.bind(ev).bind(ed)
    } else {
        query
    };

    let time = query.fetch_optional(conn).await?;

    Ok(time)
}

/// Returns the optional player from the provided login.
pub async fn get_player_from_login<'c, E: sqlx::Executor<'c, Database = sqlx::MySql>>(
    db: E,
    player_login: &str,
) -> RecordsResult<Option<Player>> {
    let r = sqlx::query_as("SELECT * FROM players WHERE login = ?")
        .bind(player_login)
        .fetch_optional(db)
        .await?;
    Ok(r)
}

/// Returns the player from the provided ID.
///
/// The return of this function isn't optional as if an ID is provided, the player most likely
/// already exists.
pub async fn get_player_from_id<'c, E: sqlx::Executor<'c, Database = sqlx::MySql>>(
    db: E,
    id: u32,
) -> RecordsResult<Player> {
    let r = sqlx::query_as("SELECT * FROM players WHERE id = ?")
        .bind(id)
        .fetch_one(db)
        .await?;
    Ok(r)
}
