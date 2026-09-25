//! What we ask [ManiaExchange] about mappacks.
//!
//! [ManiaExchange]: https://sm.mania.exchange
//!
//! A mappack is a set of maps in MX. The latter holds the list itself, who made it and when.
//! In the Obstacle API, we use mappacks in the website to compute the scores. Mappacks could also
//! be attached to event editions.
//!
//! Neither endpoint takes more than one mappack at a time, so a batch here is one request per
//! mappack instead of one request for the lot.
//!
//! Everybody waits here, unlike [the MX ID of a map](crate::maps::MapMxIds): a mappack we know
//! nothing about is a page we can't render at all, so there's nothing to answer with in the
//! meantime.

use std::collections::HashMap;
use std::time::Duration;

use records_lib::error::RecordsResult;

use crate::{
    api::{self, MxApi},
    maps::{MxIdSink, MxMap},
    polite::{Fetcher, MxQuery, Policy, Provider, Sink},
};

/// How long the answers about a mappack are kept in memory.
///
/// Nothing writes them down, so this is all we have; it stays short because a mappack does gain
/// maps while an event is being prepared, and the person adding them shouldn't have to wonder why
/// we don't see them.
const MAPPACK_TIMEOUT: Duration = Duration::from_mins(10);

/// The amount of mappacks kept in memory.
const MAX_CACHED_MAPPACKS: u64 = 512;

/// The minimum delay between two requests sent to MX about mappacks.
const MAPPACK_FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// A mappack, as MX lets us ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MappackRef {
    /// The MX ID of the mappack.
    pub mappack_id: u32,
    /// The secret of the mappack, for the ones which aren't public.
    pub secret: Option<String>,
}

/// The description MX gives of a mappack.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct MxMappackInfo {
    /// The name of whoever made the mappack.
    #[serde(rename = "Username")]
    pub username: String,
    /// The name of the mappack.
    #[serde(rename = "Name")]
    pub name: String,
    /// When the mappack was created, as MX formats it.
    #[serde(rename = "Created")]
    pub created: String,
}

/// The maps of the mappacks.
pub enum MappackTracks {}

impl MxQuery for MappackTracks {
    type Key = MappackRef;
    type Value = Vec<MxMap>;

    const NAME: &'static str = "mappack tracks";

    const POLICY: Policy = Policy {
        flush_interval: MAPPACK_FLUSH_INTERVAL,
        known_timeout: MAPPACK_TIMEOUT,
        unknown_timeout: MAPPACK_TIMEOUT,
        max_cached_keys: MAX_CACHED_MAPPACKS,
        queue_size: 32,
    };
}

impl Fetcher<MappackTracks> for MxApi {
    #[allow(clippy::manual_async_fn)]
    fn fetch<'a>(
        &'a self,
        mappacks: &'a [&'a MappackRef],
    ) -> impl Future<Output = RecordsResult<HashMap<MappackRef, Vec<MxMap>>>> + Send + 'a {
        async move {
            // This endpoint answers about one mappack, so a batch is one request per mappack.
            let requests = mappacks
                .iter()
                .map(|&mappack| async move {
                    let MappackRef { mappack_id, secret } = mappack;

                    let secret = core::fmt::from_fn(|f| match secret {
                        Some(secret) => write!(f, "?secret={secret}"),
                        None => Ok(()),
                    });

                    let tracks = api::json_or_absent::<Vec<MxMap>>(self.get(format!(
                        "https://sm.mania.exchange/api/mappack\
                         /get_mappack_tracks/{mappack_id}{secret}"
                    )))
                    .await?;

                    RecordsResult::Ok(tracks.map(|tracks| (mappack.clone(), tracks)))
                })
                .collect::<Vec<_>>();

            let results = api::gather(requests).await?;

            Ok(results.into_iter().flatten().collect())
        }
    }
}

/// The description of the mappacks.
pub enum MappackInfos {}

impl MxQuery for MappackInfos {
    type Key = u32;
    type Value = MxMappackInfo;

    const NAME: &'static str = "mappack infos";

    const POLICY: Policy = Policy {
        flush_interval: MAPPACK_FLUSH_INTERVAL,
        known_timeout: MAPPACK_TIMEOUT,
        unknown_timeout: MAPPACK_TIMEOUT,
        max_cached_keys: MAX_CACHED_MAPPACKS,
        queue_size: 32,
    };
}

impl Fetcher<MappackInfos> for MxApi {
    #[allow(clippy::manual_async_fn)]
    fn fetch<'a>(
        &'a self,
        mappack_ids: &'a [&'a u32],
    ) -> impl Future<Output = RecordsResult<HashMap<u32, MxMappackInfo>>> + Send + 'a {
        async move {
            let requests = mappack_ids
                .iter()
                .map(|&&mappack_id| async move {
                    let info = api::json_or_absent::<MxMappackInfo>(self.get(format!(
                        "https://sm.mania.exchange/api/mappack/get_info/{mappack_id}"
                    )))
                    .await?;

                    RecordsResult::Ok(info.map(|info| (mappack_id, info)))
                })
                .collect::<Vec<_>>();

            let results = api::gather(requests).await?;

            Ok(results.into_iter().flatten().collect())
        }
    }
}

/// Provides everything we need from MX about a mappack.
///
/// Its two halves are separate [queries](MxQuery), each with its own worker and its own cache, so
/// asking for both at once costs one request each, sent side by side.
#[derive(Clone)]
pub struct MappackProvider {
    tracks: Provider<MappackTracks>,
    infos: Provider<MappackInfos>,
}

impl MappackProvider {
    /// Creates the provider on top of the provided HTTP client, and spawns the workers batching
    /// the requests to the MX API.
    ///
    /// This must be called from within a Tokio runtime.
    pub fn from_client<S: MxIdSink>(client: reqwest::Client, sink: S) -> Self {
        struct WrapperSink<S>(S);
        impl<S: MxIdSink> Sink<MappackTracks> for WrapperSink<S> {
            #[allow(clippy::manual_async_fn)]
            fn store<'a>(
                &'a self,
                values: &'a HashMap<MappackRef, Vec<MxMap>>,
            ) -> impl Future<Output = RecordsResult> + Send + 'a {
                async move {
                    let values = values
                        .values()
                        .flatten()
                        .map(|map| (map.map_uid.to_owned(), map.mx_id as _))
                        .collect();
                    self.0.store(&values).await
                }
            }
        }

        let api = MxApi::new(client);
        Self {
            tracks: Provider::spawn(api.clone(), WrapperSink(sink)),
            infos: Provider::spawn(api, ()),
        }
    }

    /// Returns the maps of a mappack, waiting for MX when we don't have them yet.
    ///
    /// Returns [`None`] when MX has no such mappack, which includes a mappack whose secret we got
    /// wrong.
    pub async fn tracks(
        &self,
        mappack_id: u32,
        secret: Option<&str>,
    ) -> RecordsResult<Option<Vec<MxMap>>> {
        let mappack = MappackRef {
            mappack_id,
            secret: secret.map(str::to_owned),
        };

        Ok(self
            .tracks
            .get_or_fetch(&[&mappack])
            .await?
            .remove(&mappack))
    }

    /// Returns what MX says about a mappack, waiting for it when we don't have it yet.
    ///
    /// Returns [`None`] when MX has no such mappack.
    pub async fn info(&self, mappack_id: u32) -> RecordsResult<Option<MxMappackInfo>> {
        Ok(self
            .infos
            .get_or_fetch(&[&mappack_id])
            .await?
            .remove(&mappack_id))
    }
}
