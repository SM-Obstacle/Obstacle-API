//! This module contains anything related to in-game players in this library.

use entity::{global_event_records, global_records, players};
use sea_orm::{ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _, QuerySelect};

use crate::error::RecordsResult;
use crate::internal;
use crate::opt_event::OptEvent;

/// Returns the time of a player on a map.
pub async fn get_time_on_map<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<Option<i32>> {
    let time = match event.event {
        Some((ev, ed)) => {
            global_event_records::Entity::find()
                .filter(
                    global_event_records::Column::MapId
                        .eq(map_id)
                        .and(global_event_records::Column::RecordPlayerId.eq(player_id))
                        .and(global_event_records::Column::EventId.eq(ev.id))
                        .and(global_event_records::Column::EditionId.eq(ed.id)),
                )
                .select_only()
                .column(global_event_records::Column::Time)
                .into_tuple()
                .one(conn)
                .await?
        }
        None => {
            global_records::Entity::find()
                .filter(
                    global_records::Column::MapId
                        .eq(map_id)
                        .and(global_records::Column::RecordPlayerId.eq(player_id)),
                )
                .select_only()
                .column(global_records::Column::Time)
                .into_tuple()
                .one(conn)
                .await?
        }
    };

    Ok(time)
}

/// Returns the optional player from the provided login.
pub async fn get_player_from_login<C: ConnectionTrait>(
    conn: &C,
    login: &str,
) -> RecordsResult<Option<players::Model>> {
    let player = players::Entity::find()
        .filter(players::Column::Login.eq(login))
        .one(conn)
        .await?;
    Ok(player)
}

/// Returns the player from the provided ID.
///
/// The return of this function isn't optional as if an ID is provided, the player most likely
/// already exists.
pub async fn get_player_from_id<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
) -> RecordsResult<players::Model> {
    let player = players::Entity::find_by_id(player_id)
        .one(conn)
        .await?
        .ok_or_else(|| internal!("Player with ID {player_id} not found in get_player_from_id - this should not happen as the player is expected to exist"))?;
    Ok(player)
}
