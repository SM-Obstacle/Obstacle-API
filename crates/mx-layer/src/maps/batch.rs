//! Batching worker for the MX API.
//!
//! Fetching the MX ID of a map is an enrichment step which happens on paths that are called very
//! often, and one map at a time (a player joining a map, a GraphQL query resolving the `mxId`
//! field of a few maps, etc.). Sending one request to MX per map UID would be both slow and rude,
//! so nobody talks to MX directly: the map UIDs go through an [`mpsc`] channel to a single worker,
//! which groups them and sends at most one request every [`FLUSH_INTERVAL`].
//!
//! Nobody ever waits for MX: scheduling map UIDs is fire and forget, and the answer is written
//! down by a task of its own — the cache first, then the [`MxIdSink`]. Callers get it from the
//! cache on their next call.
//!
//! The worker is the only owner of that rhythm, and the only thing which talks to MX. It never
//! waits for MX either: a batch is sent by a task of its own, so the worker keeps taking the map
//! UIDs which come in while MX is thinking.

use core::fmt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt as _, TryStreamExt as _, stream};
use records_lib::{assert_future_send, error::RecordsResult};
use reqwest::header;
use tokio::{
    sync::mpsc,
    time::{Instant, MissedTickBehavior},
};

use super::MxIdCache;

#[cfg(test)]
mod tests;

/// The minimum delay between two requests sent to the MX API.
pub(super) const FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// The amount of batches the worker can have in its queue before its clients start to wait when
/// submitting a new one. The worker never blocks, so this is only a burst buffer.
const QUEUE_SIZE: usize = 100;

/// The amount of map UIDs sent in a single request to the MX API.
const CHUNK_SIZE: usize = 50;

/// The amount of requests to the MX API sent concurrently for a single batch.
const MAX_CONCURRENT_CHUNKS: usize = 10;

/// A way of retrieving the MX ID of some maps, identified by their UID.
///
/// This is the unbatched view of the MX API. The implementation used in production is
/// [`ReqwestMxFetcher`]; the tests use a fake one, to avoid flooding MX.
pub trait MxFetcher: Send + Sync + 'static {
    /// Fetches the MX ID of the provided map UIDs.
    ///
    /// The returned map only contains the map UIDs known by MX, so it may be smaller than the
    /// provided slice.
    fn fetch_mx_ids<'a>(
        &'a self,
        map_uids: &'a [&'a str],
    ) -> impl Future<Output = RecordsResult<HashMap<String, i32>>> + Send + 'a;
}

/// Where the MX IDs we fetched are saved.
///
/// This is how the MX IDs end up in the `mx_id` column of our maps, without this crate having to
/// know anything about our database. Nobody waits for it: it's called once the requester of a
/// batch got its answer.
pub trait MxIdSink: Send + Sync + 'static {
    /// Saves the MX ID of the provided map UIDs.
    fn store<'a>(
        &'a self,
        map_ids: &'a HashMap<String, i32>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a;
}

/// A sink which doesn't save anything, for when the MX IDs are only needed in memory.
impl MxIdSink for () {
    #[inline]
    fn store<'a>(
        &'a self,
        _: &'a HashMap<String, i32>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        std::future::ready(Ok(()))
    }
}

/// The [`MxFetcher`] implementation actually hitting the MX API.
pub struct ReqwestMxFetcher(reqwest::Client);

impl ReqwestMxFetcher {
    /// Creates a fetcher from the provided HTTP client.
    #[inline]
    pub fn new(client: reqwest::Client) -> Self {
        Self(client)
    }
}

impl MxFetcher for ReqwestMxFetcher {
    #[inline]
    fn fetch_mx_ids<'a>(
        &'a self,
        map_uids: &'a [&'a str],
    ) -> impl Future<Output = RecordsResult<HashMap<String, i32>>> + Send + 'a {
        fetch_mx_map_ids(&self.0, map_uids)
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

async fn fetch_mx_map_ids<'a>(
    client: &'a reqwest::Client,
    maps_uids: &'a [&'a str],
) -> RecordsResult<HashMap<String, i32>> {
    if maps_uids.is_empty() {
        return Ok(Default::default());
    }

    let results = assert_future_send(
        stream::iter(maps_uids.chunks(CHUNK_SIZE))
            .map(|maps_uids| async move {
                let map_uids = fmt::from_fn(|f| {
                    let mut iter = maps_uids.iter();
                    if let Some(first) = iter.next() {
                        f.write_str(first)?;
                    }
                    for map_uid in iter {
                        f.write_str(",")?;
                        f.write_str(map_uid)?;
                    }
                    Ok(())
                });

                client
                    .get(format!(
                        "https://sm.mania.exchange/api/maps\
                         ?fields=MapId,MapUid&count={CHUNK_SIZE}&uid={map_uids}"
                    ))
                    .header(header::USER_AGENT, crate::MX_USER_AGENT)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<MxMapsResult>()
                    .await
            })
            .buffer_unordered(MAX_CONCURRENT_CHUNKS)
            .try_collect::<Vec<_>>(),
    )
    .await?;

    Ok(results
        .into_iter()
        .flat_map(|result| result.Results)
        .map(|result| (result.MapUid, result.MapId))
        .collect())
}

/// Everything a batch needs once it left the worker.
struct Sending<F, S> {
    fetcher: F,
    sink: S,
    cache: MxIdCache,
}

/// Sends one batch to MX, and writes its answer down.
///
/// This runs in a task of its own, so that the worker stays free to take the next requests while
/// MX is thinking.
async fn send_batch<F: MxFetcher, S: MxIdSink>(
    sending: Arc<Sending<F, S>>,
    batch: HashSet<String>,
) {
    let results = {
        let map_uids = batch.iter().map(String::as_str).collect::<Vec<_>>();
        sending.fetcher.fetch_mx_ids(&map_uids).await
    };

    let results = match results {
        Ok(results) => results,
        Err(e) => {
            tracing::error!("couldn't fetch the MX ID of {} maps: {e}", batch.len());
            // Nothing is written down, so these map UIDs are asked again by the next call.
            return;
        }
    };

    // The cache first: the sooner it holds the answer, the fewer callers ask for it again. A map
    // UID missing from the results is one MX doesn't know, and is cached as such.
    for map_uid in batch {
        let mx_id = results.get(&map_uid).copied();
        sending.cache.insert(map_uid, mx_id).await;
    }

    if let Err(e) = sending.sink.store(&results).await {
        tracing::error!("couldn't save the MX ID of {} maps: {e}", results.len());
    }
}

/// The worker grouping the requests to the MX API. See the [module documentation](self).
async fn run_worker<F: MxFetcher, S: MxIdSink>(
    fetcher: F,
    sink: S,
    cache: MxIdCache,
    mut jobs: mpsc::Receiver<Vec<String>>,
) {
    let sending = Arc::new(Sending {
        fetcher,
        sink,
        cache,
    });

    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    // We reset the interval after each batch, so a tick we didn't have time to take is one we
    // don't want to take right after.
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // The first tick completes immediately
    interval.tick().await;

    let mut last_fetch_at = None::<Instant>;

    loop {
        let mut batch = HashSet::<String>::new();
        let mut closed = false;

        loop {
            tokio::select! {
                biased;
                map_uids = jobs.recv() => {
                    let Some(map_uids) = map_uids else {
                        closed = true;
                        break;
                    };

                    batch.extend(map_uids);

                    // Nothing was sent to MX for a while: no reason to hold this batch.
                    if last_fetch_at.is_none_or(|at| at.elapsed() >= FLUSH_INTERVAL) {
                        break;
                    }
                }
                // The window is over: what we collected leaves.
                _ = interval.tick() => break,
            }
        }

        if !batch.is_empty() {
            last_fetch_at = Some(Instant::now());
            // Our own rhythm has just moved: the interval must follow it, otherwise the next tick
            // could fire right after this batch.
            interval.reset();
            tokio::task::spawn(send_batch(Arc::clone(&sending), batch));
        }

        if closed {
            return;
        }
    }
}

/// A handle to the batching worker.
#[derive(Clone)]
pub(super) struct Batcher {
    tx: mpsc::Sender<Vec<String>>,
}

impl Batcher {
    /// Spawns the batching worker, which writes the answers of MX into `cache`, then saves them
    /// through `sink`.
    ///
    /// This must be called from within a Tokio runtime.
    pub(super) fn spawn<F: MxFetcher, S: MxIdSink>(fetcher: F, sink: S, cache: MxIdCache) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE_SIZE);
        tokio::task::spawn(run_worker(fetcher, sink, cache, rx));
        Self { tx }
    }

    /// Hands the provided map UIDs to the worker, and forgets about them.
    ///
    /// This never waits for MX: the answer goes to the cache and the sink, for the next calls to
    /// pick up. The worker never blocks either, so submitting is a matter of microseconds, even
    /// when its queue is full.
    pub(super) async fn schedule(&self, map_uids: &[&str]) {
        let map_uids = map_uids.iter().copied().map(str::to_owned).collect();

        if self.tx.send(map_uids).await.is_err() {
            tracing::error!("the MX batching worker is gone");
        }
    }
}
