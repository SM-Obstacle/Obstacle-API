//! This module contains anything related to ShootMania Obstacle maps in this library.

use core::fmt;

use entity::maps;
use sea_orm::{ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _};

use crate::error::RecordsResult;

/// Returns the map bound to the provided ID.
// TODO: remove useless wrapper
pub async fn get_map_from_id<C: ConnectionTrait>(
    conn: &C,
    map_id: u32,
) -> RecordsResult<maps::Model> {
    let map = maps::Entity::find_by_id(map_id)
        .one(conn)
        .await?
        .unwrap_or_else(|| panic!("Map with ID {map_id} not found in get_map_from_id - expected to exist in database"));
    Ok(map)
}

/// Returns the optional map from its UID.
// TODO: remove useless wrapper
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

/// Represents an item returned by a request to the MX API related to maps.
#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
pub struct MxMappackMapItem {
    /// The UID of the map.
    pub TrackUID: String,
    /// The MX ID of the map.
    pub MapID: i64,
    /// name of the map.
    pub GbxMapName: String,
    /// The login of the author.
    pub AuthorLogin: String,
}

/// Fetches the MX API to get the maps of a mappack and returns them.
///
/// ## Parameters
///
/// * `mappack_id`: the MX ID of the mappack.
/// * `secret`: an optional mappack secret.
pub async fn fetch_mx_mappack_maps(
    client: &reqwest::Client,
    mappack_id: u32,
    secret: Option<&str>,
) -> RecordsResult<Vec<MxMappackMapItem>> {
    struct SecretParam<'a>(Option<&'a str>);

    impl fmt::Display for SecretParam<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(s) = self.0 {
                write!(f, "?secret={s}")?;
            }
            Ok(())
        }
    }

    let secret = SecretParam(secret);

    client
        .get(format!(
            "https://sm.mania.exchange/api/mappack/get_mappack_tracks/{mappack_id}{secret}"
        ))
        .header("User-Agent", "obstacle (discord @ahmadbky)")
        .send()
        .await?
        .json()
        .await
        .map_err(From::from)
}
