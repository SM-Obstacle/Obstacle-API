//! What we ask [ManiaExchange] about maps.
//!
//! [ManiaExchange]: https://sm.mania.exchange
//!
//! Two questions, which are each other's mirror:
//!
//! * [`MapMxIds`] goes from a map UID to its MX ID, which we keep in the `mx_id` column of our
//!   maps table;
//! * [`MxMaps`] goes the other way, from an MX ID to the map itself: its UID, its name, its
//!   author. It's how a map MX has and we don't save it to our database.

use std::collections::HashMap;
use std::time::Duration;

use records_lib::error::RecordsResult;

use crate::{
    api::{self, MxApi},
    polite::{Fetcher, MxQuery, MxStatus, Policy, Provider, Sink},
};

#[cfg(test)]
mod tests;

/// A map, as MX describes it.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct MxMap {
    /// The MX ID of the map.
    #[serde(rename = "MapID")]
    pub mx_id: i64,
    /// The UID of the map, as the game knows it.
    #[serde(rename = "TrackUID")]
    pub map_uid: String,
    /// The name of the map.
    #[serde(rename = "GbxMapName")]
    pub name: String,
    /// The login of the author of the map.
    #[serde(rename = "AuthorLogin")]
    pub author_login: String,
}

// --------
// --- The MX ID of a map, from its UID
// --------

/// The delay after which we ask MX again about a map it said it doesn't know.
///
/// This answers "has this map been uploaded to MX since?", which happens once, at a moment that
/// has nothing to do with our traffic: checking often would only flood MX. Whoever knows better
/// than us that a map just landed on MX can ask for it explicitly instead of waiting for this.
const MX_ID_UNKNOWN_TIMEOUT: Duration = Duration::from_hours(24);

/// The delay after which we drop an MX ID we know.
///
/// It's much shorter than [`MX_ID_UNKNOWN_TIMEOUT`], which looks backwards but isn't: an MX ID we
/// know is saved in our database by the [`MxIdSink`], and that's where it's read from afterwards.
/// These entries only cover the moment between the answer of MX and the write, so keeping them
/// longer buys nothing. The entries that carry the load are the negative ones, meaning maps which
/// are *not* on MX keep a null column forever, so they come back on every page that shows them.
const MX_ID_KNOWN_TIMEOUT: Duration = Duration::from_hours(2);

/// The amount of map UIDs sent in a single HTTP request to the MX API.
const MX_ID_CHUNK_SIZE: usize = 50;

/// The MX ID of the maps, identified by their UID.
///
/// Nobody waits for this one: see [`MxIdProvider::get_mx_ids_of_map_uids`].
pub enum MapMxIds {}

impl MxQuery for MapMxIds {
    type Key = String;
    type Value = i32;

    const NAME: &'static str = "map MX IDs";

    const POLICY: Policy = Policy {
        flush_interval: Duration::from_secs(2),
        known_timeout: MX_ID_KNOWN_TIMEOUT,
        unknown_timeout: MX_ID_UNKNOWN_TIMEOUT,
        // This is the query our own traffic goes through, so it's the one worth remembering.
        max_cached_keys: 50_000,
        queue_size: 100,
    };
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct MxMapIdResult {
    MapId: i32,
    MapUid: String,
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct MxMapIdsResult {
    Results: Vec<MxMapIdResult>,
}

impl Fetcher<MapMxIds> for MxApi {
    #[allow(clippy::manual_async_fn)]
    fn fetch<'a>(
        &'a self,
        map_uids: &'a [&'a String],
    ) -> impl Future<Output = RecordsResult<HashMap<String, i32>>> + Send + 'a {
        async move {
            // This endpoint takes several map UIDs at once, so a batch costs one request per chunk
            // instead of one per map.
            let requests = map_uids
                .chunks(MX_ID_CHUNK_SIZE)
                .map(|map_uids| {
                    let map_uids = map_uids.iter().map(|uid| uid.as_str()).collect::<Vec<_>>();
                    let map_uids = map_uids.join(",");

                    api::json::<MxMapIdsResult>(self.get(format!(
                        "https://sm.mania.exchange/api/maps\
                         ?fields=MapId,MapUid&count={MX_ID_CHUNK_SIZE}&uid={map_uids}"
                    )))
                })
                .collect::<Vec<_>>();

            let results = api::gather(requests).await?;

            Ok(results
                .into_iter()
                .flat_map(|result| result.Results)
                .map(|result| (result.MapUid, result.MapId))
                .collect())
        }
    }
}

/// Where the MX IDs we fetched are saved, which is the `mx_id` column of our maps.
pub trait MxIdSink: Sink<MapMxIds> {}

impl<T: Sink<MapMxIds>> MxIdSink for T {}

/// What we know about the MX ID of a map, without asking MX.
pub type MxIdStatus = MxStatus<i32>;

/// Gives the MX ID of the maps, identified by their UID.
///
/// See the [politeness layer](crate::polite) for what it does with the map UIDs it doesn't have an
/// answer for.
pub type MxIdProvider = Provider<MapMxIds>;

impl MxIdProvider {
    /// Creates the provider on top of the provided HTTP client, and spawns the worker batching the
    /// requests to the MX API.
    ///
    /// This must be called from within a Tokio runtime.
    #[inline]
    pub fn from_client<S: MxIdSink>(client: reqwest::Client, sink: S) -> Self {
        Self::spawn(MxApi::new(client), sink)
    }

    /// Returns the MX ID of the provided maps, identified by their UID.
    ///
    /// This never waits for the MX API. The map UIDs we know nothing about are handed to the
    /// batching worker, which fetches them in the background and saves them, so they're missing
    /// from the returned map even though MX may know them. Calling this again a few seconds later
    /// gives them.
    ///
    /// A map UID missing from the returned map is therefore to be read as "we have no MX ID for
    /// this map *right now*".
    #[inline]
    pub async fn get_mx_ids_of_map_uids(&self, map_uids: &[&str]) -> HashMap<String, i32> {
        self.get_or_schedule(map_uids).await
    }

    /// Asks MX about a map right now, and saves what it answers.
    ///
    /// Unlike [`get_mx_ids_of_map_uids`](Self::get_mx_ids_of_map_uids), this waits for MX, skips
    /// the batching window, and ignores a cached "MX doesn't have this map": it's meant for
    /// someone who explicitly asks us to look again, and who knows better than our cache does. It
    /// therefore costs a request to the MX API every time, and mustn't be driven by our own
    /// traffic.
    ///
    /// Returns [`None`] when MX doesn't have this map.
    #[inline]
    pub async fn force_fetch(&self, map_uid: &str) -> RecordsResult<Option<i32>> {
        Ok(self.force(&[map_uid]).await?.remove(map_uid))
    }
}

// --------
// --- The map behind an MX ID
// --------

/// The amount of MX IDs sent in a single HTTP request to the MX API.
const MX_MAP_CHUNK_SIZE: usize = 10;

/// The maps of MX, identified by their MX ID.
///
/// Somebody always waits for this one: see [`MxMapProvider::get_maps_of_mx_ids`].
pub enum MxMaps {}

impl MxQuery for MxMaps {
    type Key = i64;
    type Value = MxMap;

    const NAME: &'static str = "MX maps";

    const POLICY: Policy = Policy {
        flush_interval: Duration::from_secs(2),
        known_timeout: Duration::from_hours(2),
        // Nothing here is written down anywhere, and an MX ID which points at no map is an MX ID
        // somebody mistyped: there's no point in asking again soon.
        unknown_timeout: Duration::from_hours(2),
        max_cached_keys: 10_000,
        queue_size: 100,
    };
}

impl Fetcher<MxMaps> for MxApi {
    #[allow(clippy::manual_async_fn)]
    fn fetch<'a>(
        &'a self,
        mx_ids: &'a [&'a i64],
    ) -> impl Future<Output = RecordsResult<HashMap<i64, MxMap>>> + Send + 'a {
        async move {
            let requests = mx_ids
                .chunks(MX_MAP_CHUNK_SIZE)
                .map(|mx_ids| {
                    let mx_ids = mx_ids
                        .iter()
                        .map(|mx_id| mx_id.to_string())
                        .collect::<Vec<_>>()
                        .join(",");

                    api::json::<Vec<MxMap>>(self.get(format!(
                        "https://sm.mania.exchange/api/maps/get_map_info/multi/{mx_ids}"
                    )))
                })
                .collect::<Vec<_>>();

            let results = api::gather(requests).await?;

            Ok(results
                .into_iter()
                .flatten()
                .map(|map| (map.mx_id, map))
                .collect())
        }
    }
}

/// Gives the maps of MX, identified by their MX ID.
pub type MxMapProvider = Provider<MxMaps>;

impl MxMapProvider {
    /// Creates the provider on top of the provided HTTP client, and spawns the worker batching the
    /// requests to the MX API.
    ///
    /// This must be called from within a Tokio runtime.
    #[inline]
    pub fn from_client(client: reqwest::Client) -> Self {
        Self::spawn(MxApi::new(client), ())
    }

    /// Returns the maps behind the provided MX IDs, waiting for MX when we don't have them yet.
    ///
    /// Whoever asks this is about to write these maps down, so they can't be told to come back
    /// later. The wait is still polite: the MX IDs join the next batching window like any other
    /// traffic of ours, deduplicated with whatever else is asked for at that moment.
    ///
    /// The MX IDs MX has no map for are missing from the returned map.
    pub async fn get_maps_of_mx_ids(&self, mx_ids: &[i64]) -> RecordsResult<HashMap<i64, MxMap>> {
        let mx_ids = mx_ids.iter().collect::<Vec<_>>();
        self.get_or_fetch(&mx_ids).await
    }
}
