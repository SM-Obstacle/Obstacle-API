use core::fmt;
use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::{StreamExt as _, TryStreamExt as _, stream};
use records_lib::{assert_future_send, error::RecordsResult};
use reqwest::header;
use tokio::{
    sync::{Mutex, Notify, mpsc, oneshot},
    time::Instant,
};

#[cfg(test)]
mod tests;

const CACHE_TIMEOUT: Duration = Duration::from_mins(5);
const CACHE_PURGE_INTERVAL: Duration = Duration::from_hours(2);

const DEBOUNCE_FETCH_INTERVAL: Duration = Duration::from_secs(30);

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
    let secret = fmt::from_fn(|f| {
        if let Some(s) = secret {
            write!(f, "?secret={s}")?;
        }
        Ok(())
    });

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

trait Sealed {}

// This trait is used just for us to test the caching mechanism
#[allow(private_bounds)]
pub trait MxSource: Sealed {
    #[allow(async_fn_in_trait)]
    async fn fetch_mx_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>>;
}

pub struct DefaultMxSource {
    broker_client: BrokerClient,
}

impl DefaultMxSource {
    fn from_client(client: reqwest::Client) -> Self {
        let (tx, rx) = mpsc::channel(100);
        tokio::task::spawn(broker_fetch(client, rx));
        Self {
            broker_client: BrokerClient { tx },
        }
    }
}

impl Sealed for DefaultMxSource {}
impl MxSource for DefaultMxSource {
    #[inline]
    async fn fetch_mx_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        self.broker_client.get_map_ids(map_uids).await
    }
}

async fn fetch_mx_map_ids(
    client: &reqwest::Client,
    maps_uids: &[&str],
) -> RecordsResult<HashMap<String, i32>> {
    const CHUNK_SIZE: usize = 50;

    if maps_uids.is_empty() {
        return Ok(Default::default());
    }

    let mut chunks = assert_future_send(stream::iter(maps_uids.chunks(CHUNK_SIZE).enumerate())
        .map(|(chunk_idx, maps_uids)| async move {
            let map_uids = fmt::from_fn(|f| {
                let mut iter = maps_uids.iter();
                if let Some(first) = iter.next() {
                    fmt::Display::fmt(first, f)?;
                }
                for item in iter {
                    f.write_str(",")?;
                    fmt::Display::fmt(item, f)?;
                }
                Ok(())
            });

            match client
                .get(format!(
                    "https://sm.mania.exchange/api/maps?fields=MapId,MapUid&count={CHUNK_SIZE}&uid={map_uids}"
                ))
                .header(header::USER_AGENT, crate::MX_USER_AGENT)
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

#[derive(Debug)]
struct CachedMxMapId {
    map_mx_id: Option<i32>,
    at: Instant,
}

impl CachedMxMapId {
    fn is_expired(&self) -> bool {
        self.at.elapsed() > CACHE_TIMEOUT
    }

    fn must_be_purged(&self) -> bool {
        self.at.elapsed() > CACHE_PURGE_INTERVAL
    }
}

#[derive(Clone, Default)]
struct CachedMapUids {
    inner: Arc<Mutex<HashMap<String, CachedMxMapId>>>,
}

async fn purge_old_cache(cached: CachedMapUids, notify: Arc<Notify>) {
    let mut timer = tokio::time::interval(CACHE_PURGE_INTERVAL);
    // The first tick completes immediately
    timer.tick().await;
    loop {
        timer.tick().await;
        let mut lock = cached.inner.lock().await;
        let keys_to_remove = lock
            .iter()
            .filter_map(|(map_uid, cached)| {
                if cached.must_be_purged() {
                    Some(map_uid.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for key in keys_to_remove {
            lock.remove(&key);
        }
        notify.notify_waiters();
    }
}

struct BrokerRequest {
    map_uids: Vec<String>,
    reply_tx: oneshot::Sender<RecordsResult<HashMap<String, i32>>>,
}

async fn broker_fetch(client: reqwest::Client, mut jobs: mpsc::Receiver<BrokerRequest>) {
    let Some(BrokerRequest { map_uids, reply_tx }) = jobs.recv().await else {
        return;
    };

    let result = fetch_mx_map_ids(
        &client,
        map_uids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .await;

    let _ = reply_tx.send(result);

    let mut collected_map_uids = Vec::new();
    let mut last_fetch_at = Instant::now();

    loop {
        let Some(BrokerRequest { map_uids, reply_tx }) = jobs.recv().await else {
            return;
        };
        collected_map_uids.extend_from_slice(&map_uids);

        if last_fetch_at.elapsed() < DEBOUNCE_FETCH_INTERVAL {
            let _ = reply_tx.send(Ok(Default::default()));
            continue;
        }

        let result = fetch_mx_map_ids(
            &client,
            collected_map_uids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await;
        let _ = reply_tx.send(result);

        collected_map_uids.clear();
        last_fetch_at = Instant::now();
    }
}

struct BrokerClient {
    tx: mpsc::Sender<BrokerRequest>,
}

impl BrokerClient {
    async fn get_map_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        let (tx, rx) = oneshot::channel();
        let request = BrokerRequest {
            map_uids: map_uids.iter().copied().map(str::to_owned).collect(),
            reply_tx: tx,
        };

        if self.tx.send(request).await.is_err() {
            return Ok(Default::default());
        }

        match rx.await {
            Ok(res) => res,
            Err(_) => Ok(Default::default()),
        }
    }
}

struct SharedCachedMxMapIds<S: MxSource> {
    source: S,
    cached: CachedMapUids,
}

pub struct CachedMxMapIds<S: MxSource = DefaultMxSource>(Arc<SharedCachedMxMapIds<S>>);

impl Clone for CachedMxMapIds {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl CachedMxMapIds {
    pub fn from_client(client: reqwest::Client) -> Self {
        Self::from_source(DefaultMxSource::from_client(client))
    }
}

impl<S: MxSource> CachedMxMapIds<S> {
    fn from_source(source: S) -> Self {
        let (this, _) = Self::from_source_with_notify(source);
        this
    }

    // Used for testing
    fn from_source_with_notify(source: S) -> (Self, Arc<Notify>) {
        let cached = CachedMapUids::default();
        let notify = Arc::new(Notify::new());
        tokio::task::spawn(purge_old_cache(Clone::clone(&cached), Arc::clone(&notify)));
        (
            Self(Arc::new(SharedCachedMxMapIds { source, cached })),
            notify,
        )
    }

    /// Fetches the MX ID of the provided maps, identified by their UID, from the MX API.
    pub async fn get_map_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        let mut locked_map = self.0.cached.inner.lock().await;

        // The purpose of caching the fetch of the MX IDs is mainly because if MX didn't return
        // any ID for a given map UID, it probably won't return one for the next minutes.
        //
        // Therefore, we also don't re-fetch the MX IDs of the maps UIDs when we previously got
        // their MX IDs, because the MX ID will probably not change for a given map UID.
        //
        // Furthermore, with the cache purge, there should mainly be map UIDs with no MX ID in
        // the internal hash map, since this method should be called after having checked that we
        // didn't save the MX ID in the API DB.

        // 1. We filter the map UIDs we want to fetch, meaning those that we haven't cached yet,
        //    or whose cache is too old and without MX ID. If every map UID is already cached, then
        //    this is empty.
        let map_uids_to_fetch = map_uids
            .iter()
            .copied()
            .filter(|map_uid| !locked_map.contains_key(*map_uid))
            .chain(locked_map.iter().filter_map(|(map_uid, cached)| {
                if map_uids.contains(&map_uid.as_str())
                    && cached.is_expired()
                    && cached.map_mx_id.is_none()
                {
                    Some(map_uid.as_str())
                } else {
                    None
                }
            }))
            .collect::<Vec<_>>();

        // 2. We fetch the MX IDs of the requested map UIDs, and update the cached based on the fetch
        //    result. This step is noop if every map UID is already cached.
        let fetch_result = self.0.source.fetch_mx_ids(&map_uids_to_fetch).await?;
        let now = Instant::now();
        let updated_cache = fetch_result
            .iter()
            .map(|(map_uid, mx_id)| {
                (
                    map_uid.clone(),
                    CachedMxMapId {
                        at: now,
                        map_mx_id: Some(*mx_id),
                    },
                )
            })
            .chain(
                map_uids_to_fetch
                    .into_iter()
                    .map(str::to_owned)
                    // Need to collect first to not keep the borrow on lock
                    .collect::<Vec<_>>()
                    .into_iter()
                    .filter(|map_uid| !fetch_result.contains_key(map_uid))
                    .map(|map_uid| {
                        (
                            map_uid,
                            CachedMxMapId {
                                at: now,
                                map_mx_id: None,
                            },
                        )
                    }),
            );
        for (map_uid, cached) in updated_cache {
            locked_map.insert(map_uid, cached);
        }

        // 3. Map our internal hash map to the returned one. If every map UID was cached, this is
        //    really what's going to be returned.
        let result = locked_map
            .iter()
            .filter(|(map_uid, _)| map_uids.contains(&map_uid.as_str()))
            .filter_map(|(map_uid, cached)| cached.map_mx_id.map(|mx_id| (map_uid.clone(), mx_id)))
            .collect();

        Ok(result)
    }
}
