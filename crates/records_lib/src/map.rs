//! This module contains anything related to ShootMania Obstacle maps in this library.

use core::fmt;
use std::collections::HashMap;

use entity::maps;
use futures::{StreamExt, TryStreamExt, stream};
use sea_orm::{ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _};

use crate::{assert_future_send, error::RecordsResult, internal};

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

struct Separated<'a, T, U>
where
    U: ?Sized,
{
    list: &'a [T],
    sep: &'a U,
}

impl<'a, T> From<&'a [T]> for Separated<'a, T, str> {
    fn from(value: &'a [T]) -> Self {
        Self {
            list: value,
            sep: ",",
        }
    }
}

impl<T, U> fmt::Display for Separated<'_, T, U>
where
    T: fmt::Display,
    U: fmt::Display + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.list.iter();
        if let Some(first) = iter.next() {
            fmt::Display::fmt(first, f)?;
        }
        for item in iter {
            fmt::Display::fmt(&self.sep, f)?;
            fmt::Display::fmt(item, f)?;
        }
        Ok(())
    }
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct MxMapIdResult {
    MapId: i32,
    MapUid: String,
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct MxMapsResult {
    Results: Vec<MxMapIdResult>,
}

/// Fetches the MX ID of the provided maps, identified by their UID, from the MX API.
pub async fn fetch_mx_map_ids(
    client: &reqwest::Client,
    maps_uids: &[&str],
) -> RecordsResult<HashMap<String, i32>> {
    const CHUNK_SIZE: usize = 50;

    let mut chunks = assert_future_send(stream::iter(maps_uids.chunks(CHUNK_SIZE).enumerate())
        .map(|(chunk_idx, maps_uids)| async move {
            let map_uids = Separated::from(maps_uids);
            match client
                .get(format!(
                    "https://sm.mania.exchange/api/maps?fields=MapId,MapUid&count={CHUNK_SIZE}&uid={map_uids}"
                ))
                .send()
                .await
            {
                Ok(res) => match res.json::<MxMapsResult>().await {
                    Ok(res) => Ok((chunk_idx, res)),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            }
        }).buffer_unordered(10).try_collect::<Vec<_>>()).await?;

    chunks.sort_by_key(|(chunk_idx, _)| *chunk_idx);

    let map_id_results = chunks
        .into_iter()
        .flat_map(|(_, results)| results.Results)
        .map(|result| (result.MapUid, result.MapId))
        .collect();

    Ok(map_id_results)
}
