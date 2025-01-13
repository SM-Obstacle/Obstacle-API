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

use crate::{
    context::{HasEditionId, HasEventHandle, HasEventId, HasMapUid, HasPlayerLogin},
    error::{RecordsError, RecordsResult},
    event, map, models, player,
};

/// Returns the event in the database bound to the provided event handle.
pub async fn have_event_handle<C: HasEventHandle>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<models::Event> {
    event::get_event_by_handle(conn, ctx.get_event_handle())
        .await?
        .ok_or_else(|| RecordsError::EventNotFound(ctx.get_event_handle().to_owned()))
}

/// Returns the event and its edition in the database bound to the provided IDs.
// FIXME: although this function is called when we know the edition exists,
// do we have to get rid of the `fetch_one` method usage?
pub async fn have_event_edition_from_ids(
    conn: &mut sqlx::MySqlConnection,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<(models::Event, models::EventEdition)> {
    let event = sqlx::query_as("select * from event where id = ?")
        .bind(event_id)
        .fetch_one(&mut *conn)
        .await?;

    let edition = sqlx::query_as("select * from event_edition where event_id = ? and id = ?")
        .bind(event_id)
        .bind(edition_id)
        .fetch_one(conn)
        .await?;

    Ok((event, edition))
}

/// Returns the event and its edition in the database bound to the provided handles and edition ID.
pub async fn have_event_edition<C>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<(models::Event, models::EventEdition)>
where
    C: HasEventHandle + HasEditionId,
{
    let event = have_event_handle(conn, &ctx).await?;
    let ctx = ctx.with_event(&event);

    let Some(event_edition) =
        event::get_edition_by_id(conn, ctx.get_event_id(), ctx.get_edition_id()).await?
    else {
        return Err(RecordsError::EventEditionNotFound(
            ctx.get_event_handle().to_string(),
            ctx.get_edition_id(),
        ));
    };

    Ok((event, event_edition))
}

/// Returns the player in the database bound to the provided login.
pub async fn have_player<C: HasPlayerLogin>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<models::Player> {
    player::get_player_from_login(conn, ctx.get_player_login())
        .await?
        .ok_or_else(|| RecordsError::PlayerNotFound(ctx.get_player_login().to_string()))
}

/// Returns the map in the database bound to the provided map UID.
pub async fn have_map<C: HasMapUid>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<models::Map> {
    map::get_map_from_uid(conn, ctx.get_map_uid())
        .await?
        .ok_or_else(|| RecordsError::MapNotFound(ctx.get_map_uid().to_owned()))
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
pub async fn have_event_edition_with_map<C>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<(models::Event, models::EventEdition, event::EventMap)>
where
    C: HasEventHandle + HasEditionId + HasMapUid,
{
    let (event, event_edition) = have_event_edition(conn, &ctx).await?;
    let ctx = ctx.with_event_edition(&event, &event_edition);

    let map = event::get_map_in_edition(conn, &ctx)
        .await?
        .ok_or_else(|| {
            RecordsError::MapNotInEventEdition(
                ctx.get_map_uid().to_string(),
                ctx.get_event_handle().to_string(),
                event_edition.id,
            )
        })?;

    Ok((event, event_edition, map))
}
