//! This module contains anything related to ShootMania Obstacle maps in this library.

use core::fmt;

use sqlx::MySqlConnection;

use crate::{error::RecordsResult, models::Map};

pub async fn get_map_from_id(db: &mut MySqlConnection, map_id: u32) -> RecordsResult<Map> {
    let r = sqlx::query_as("select * from maps where id = ?")
        .bind(map_id)
        .fetch_one(db)
        .await?;
    Ok(r)
}

/// Returns the optional map from its UID.
pub async fn get_map_from_uid(
    db: &mut MySqlConnection,
    map_uid: &str,
) -> RecordsResult<Option<Map>> {
    let r = sqlx::query_as("SELECT * FROM maps WHERE game_id = ?")
        .bind(map_uid)
        .fetch_optional(db)
        .await?;
    Ok(r)
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
