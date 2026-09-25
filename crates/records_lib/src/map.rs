//! This module contains anything related to ShootMania Obstacle maps in this library.

use entity::maps;
use sea_orm::{ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _};

use crate::{error::RecordsResult, internal};

/// Returns the map bound to the provided ID.
pub async fn get_map_from_id<C: ConnectionTrait>(
    conn: &C,
    map_id: u32,
) -> RecordsResult<maps::Model> {
    let map = maps::Entity::find_by_id(map_id)
        .one(conn)
        .await?
        .ok_or_else(|| {
            internal!(
                "Map with ID {map_id} not found in get_map_from_id - expected to exist in database"
            )
        })?;
    Ok(map)
}

/// Returns the optional map from its UID.
pub async fn get_map_from_uid<C: ConnectionTrait>(
    conn: &C,
    map_uid: &str,
) -> RecordsResult<Option<maps::Model>> {
    let map = maps::Entity::find()
        .filter(maps::Column::GameId.eq(map_uid))
        .one(conn)
        .await?;
    Ok(map)
}
