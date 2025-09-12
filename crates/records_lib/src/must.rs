//! This module contains utility functions used to retrieve some objects from the database,
//! that must exist. This is why it is called `must`.
//!
//! This module is used when a route is called at a point where something must be already registered
//! in the database, like a player, a map, an event, etc.
//!
//! Unlike the Rust conventions, when such an object doesn't exist, the returned value isn't
//! `Option::None` but the corresponding error (for example, `RecordsError::PlayerNotFound`).
//! This makes the code cleaner thanks to the [`Try`](std::ops::Try) trait syntax, because at some point
//! we most likely want things to be already existing, without checking it repeatedly
//! and returning the error to the client.

use entity::{event as event_entity, event_edition, maps, players};
use sea_orm::{ConnectionTrait, EntityTrait};

use crate::{
    error::{RecordsError, RecordsResult},
    event, internal, map, player,
};

/// Returns the event in the database bound to the provided event handle.
pub async fn have_event_handle<C: ConnectionTrait>(
    conn: &C,
    event_handle: &str,
) -> RecordsResult<event_entity::Model> {
    event::get_event_by_handle(conn, event_handle)
        .await?
        .ok_or_else(|| RecordsError::EventNotFound(event_handle.to_owned()))
}

/// Returns the event and its edition in the database bound to the provided IDs.
// FIXME: although this function is called when we know the edition exists,
// do we have to get rid of the `fetch_one` method usage?
pub async fn have_event_edition_from_ids<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<(event_entity::Model, event_edition::Model)> {
    let event = event_entity::Entity::find_by_id(event_id)
        .one(conn)
        .await?
        .ok_or_else(|| {
            internal!("have_event_edition_from_ids: Event with ID {event_id} must be in database")
        })?;

    let edition = event_edition::Entity::find_by_id((event_id, edition_id)).one(conn).await?
        .ok_or_else(|| internal!("Event edition with event ID {event_id} and edition ID {edition_id} must be in database"))?;

    Ok((event, edition))
}

/// Returns the event and its edition in the database bound to the provided handles and edition ID.
pub async fn have_event_edition<C: ConnectionTrait>(
    conn: &C,
    event_handle: &str,
    edition_id: u32,
) -> RecordsResult<(event_entity::Model, event_edition::Model)> {
    let event = have_event_handle(conn, event_handle).await?;

    let Some(event_edition) = event::get_edition_by_id(conn, event.id, edition_id).await? else {
        return Err(RecordsError::EventEditionNotFound(
            event_handle.to_string(),
            edition_id,
        ));
    };

    Ok((event, event_edition))
}

/// Returns the player in the database bound to the provided login.
pub async fn have_player<C: ConnectionTrait>(
    conn: &C,
    login: &str,
) -> RecordsResult<players::Model> {
    player::get_player_from_login(conn, login)
        .await?
        .ok_or_else(|| RecordsError::PlayerNotFound(login.to_string()))
}

/// Returns the map in the database bound to the provided map UID.
pub async fn have_map<C: ConnectionTrait>(conn: &C, map_uid: &str) -> RecordsResult<maps::Model> {
    map::get_map_from_uid(conn, map_uid)
        .await?
        .ok_or_else(|| RecordsError::MapNotFound(map_uid.to_owned()))
}

/// Returns the event and its edition bound to their IDs and that contain a specific map.
///
/// ## Parameters
///
/// * `map_uid`: the UID of the map.
/// * `event_handle`: the handle of the event.
/// * `edition_id`: the ID of its edition.
///
/// ## Return
///
/// This function returns the event with its edition, and the the corresponding map
/// bound to the event.
///
/// For example, for the Benchmark, if the given map UID is `X`, the returned map will be the one
/// with the UID `X_benchmark`. If the given map UID is already `X_benchmark`, it will
/// simply be the corresponding map.
pub async fn have_event_edition_with_map<C: ConnectionTrait>(
    conn: &C,
    map_uid: &str,
    event_handle: &str,
    edition_id: u32,
) -> RecordsResult<(event_entity::Model, event_edition::Model, event::EventMap)> {
    let (event, event_edition) = have_event_edition(conn, event_handle, edition_id).await?;

    let map = event::get_map_in_edition(conn, map_uid, event.id, event_edition.id)
        .await?
        .ok_or_else(|| {
            RecordsError::MapNotInEventEdition(
                map_uid.to_string(),
                event_handle.to_string(),
                event_edition.id,
            )
        })?;

    Ok((event, event_edition, map))
}
