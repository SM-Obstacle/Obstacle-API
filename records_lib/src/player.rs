//! This module contains anything related to in-game players in this library.

use crate::context::{HasMapId, HasPlayerId};
use crate::{error::RecordsResult, models::Player};

/// Returns the time of a player on a map.
pub async fn get_time_on_map<C>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<Option<i32>>
where
    C: HasPlayerId + HasMapId,
{
    let builder = ctx.sql_frag_builder();

    let mut q = sqlx::QueryBuilder::new("select time from ");
    builder
        .push_event_view_name(&mut q, "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" and record_player_id = ")
        .push_bind(ctx.get_player_id());
    let query = builder.push_event_filter(&mut q, "r").build_query_scalar();

    let time = query.fetch_optional(conn).await?;

    Ok(time)
}

/// Returns the optional player from the provided login.
pub async fn get_player_from_login(
    conn: &mut sqlx::MySqlConnection,
    login: &str,
) -> RecordsResult<Option<Player>> {
    let r = sqlx::query_as("SELECT * FROM players WHERE login = ?")
        .bind(login)
        .fetch_optional(conn)
        .await?;
    Ok(r)
}

/// Returns the player from the provided ID.
///
/// The return of this function isn't optional as if an ID is provided, the player most likely
/// already exists.
pub async fn get_player_from_id(
    conn: &mut sqlx::MySqlConnection,
    player_id: u32,
) -> RecordsResult<Player> {
    let r = sqlx::query_as("SELECT * FROM players WHERE id = ?")
        .bind(player_id)
        .fetch_one(conn)
        .await?;
    Ok(r)
}
