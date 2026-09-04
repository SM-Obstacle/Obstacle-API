//! Everything about the MX ID of the maps: the ID [ManiaExchange] gives to a map, which the
//! website shows as a link and which we keep in the `mx_id` column of our maps.
//!
//! [ManiaExchange]: https://sm.mania.exchange
//!
//! Getting the MX ID of a map means asking ManiaExchange, which is somebody else's API: it can be
//! slow, it can be down, and it shouldn't be flooded. Everything in this module follows from three
//! rules that come out of that.
//!
//! **Nobody waits for MX.** [`MxIdProvider::get_mx_ids_of_map_uids`] answers with what we already
//! know and hands the rest to a background worker, so a map UID missing from its answer means
//! "we have no MX ID for this map *right now*", not "this map isn't on MX". Its callers are on
//! paths which must stay fast: the game asking about the map a player just joined, and the website
//! rendering a list of maps server-side.
//!
//! **MX is asked at most once every 5 seconds**, whatever our own traffic is. The worker groups the
//! map UIDs handed to it during that window into a single deduplicated batch. Without it, every page
//! of the website showing maps we haven't resolved yet would be a request to MX.
//!
//! **An answer of MX is remembered**, so we don't ask twice. An MX ID we got is written to our own
//! database — see [`MxIdSink`] — and read from there afterwards. A map MX doesn't have is remembered
//! in memory only, and asked again a day later: a map is uploaded to MX once, at a moment which has
//! nothing to do with our traffic.
//!
//! Everything talking to the outside is behind a trait — [`MxFetcher`] for the MX API, [`MxIdSink`]
//! for our database — so the tests of this module need neither.
//!
//! # The flow
#![doc = simple_mermaid::mermaid!("../docs/flow.mmd")]
//! Two details of that flow are worth spelling out, because they're what keeps everybody free:
//!
//! * the worker **spawns** the batch instead of awaiting it, so it keeps taking map UIDs while MX is
//!   thinking, instead of making the next callers queue behind a request that isn't theirs;
//! * the fetch task writes the cache **before** the database, so the answer is visible to the other
//!   callers as soon as possible; nobody waits for the `UPDATE`.
//!
//! When a batch fails, nothing is written down at all. The map UIDs are simply asked again by the next
//! call, and are never mistaken for maps MX doesn't have.
//!
//! # The two lifetimes
//!
//! The cached answers don't age the same way, which the cache turns into a per-entry expiration:
//!
//! | what we cached | expires after | why |
//! |---|---|---|
//! | an MX ID | 2 hours | it never changes, but it's in our database too, and that's where it's read from |
//! | "MX doesn't have this map" | 24 hours | it may be uploaded one day, but that has nothing to do with our traffic |
//!
//! The second one being much longer than the first looks backwards, and isn't: a map whose `mx_id`
//! column is filled never comes back here, so keeping its entry longer buys nothing. The entries that
//! carry the load are the negative ones — maps which are *not* on MX keep a null column forever, so
//! they come back on every page that shows them.
//!
//! # Telling "not on MX" from "not asked yet"
//!
//! Both leave a map UID out of [`MxIdProvider::get_mx_ids_of_map_uids`]'s answer, and the website
//! has to tell them apart: one deserves nothing on the page, the other deserves a button to look
//! again. [`MxIdProvider::status_of`] reports what we have, without asking MX and without scheduling
//! anything:
#![doc = simple_mermaid::mermaid!("../docs/status.mmd")]
//! Note that a map UID handed to the worker is still `Unknown` for the rest of that call: its batch
//! hasn't come back yet. A map nobody ever asked about therefore always reads `Unknown` the first
//! time, even when MX has it.

use core::fmt;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use moka::{Expiry, future::Cache};
use records_lib::error::RecordsResult;

mod batch;

#[cfg(test)]
mod fake_mx;
#[cfg(test)]
mod tests;

pub use batch::{MxFetcher, MxIdSink, ReqwestMxFetcher};

use batch::Batcher;

/// The delay after which we ask MX again about a map it said it doesn't know.
///
/// This answers "has this map been uploaded to MX since?", which happens once, at a moment that
/// has nothing to do with our traffic: checking often would only flood MX. Whoever knows better
/// than us that a map just landed on MX can ask for it explicitly instead of waiting for this.
const UNKNOWN_TIMEOUT: Duration = Duration::from_hours(24);
/// The delay after which we drop an MX ID we know.
///
/// It's much shorter than [`UNKNOWN_TIMEOUT`], which looks backwards but isn't: an MX ID we know
/// is saved in our database, and that's where it's read from afterwards. These entries only cover
/// the moment between the answer of MX and the write, so keeping them longer buys nothing.
const KNOWN_TIMEOUT: Duration = Duration::from_hours(2);
/// The amount of map UIDs kept in memory. The least recently used ones are dropped past that.
const MAX_CACHED_MAP_UIDS: u64 = 50_000;

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

/// What we know about the MX ID of a map, without asking MX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MxIdStatus {
    /// MX gave us this ID.
    Known(i32),
    /// MX told us it doesn't know this map, recently enough for us to still believe it.
    NotOnMx,
    /// We have no answer for this map: either we never asked, or the last one expired. A batch may
    /// be on its way.
    Unknown,
}

/// What MX told us about the map UIDs: their MX ID, or [`None`] for the ones MX doesn't know.
///
/// The batching worker is its only writer; the [`MxIdProvider`]s only read it.
type MxIdCache = Cache<String, Option<i32>>;

/// How long an answer of MX is worth keeping.
struct MxIdExpiry;

impl Expiry<String, Option<i32>> for MxIdExpiry {
    fn expire_after_create(&self, _: &String, mx_id: &Option<i32>, _: Instant) -> Option<Duration> {
        Some(match mx_id {
            Some(_) => KNOWN_TIMEOUT,
            None => UNKNOWN_TIMEOUT,
        })
    }

    fn expire_after_update(
        &self,
        map_uid: &String,
        mx_id: &Option<i32>,
        updated_at: Instant,
        _: Option<Duration>,
    ) -> Option<Duration> {
        // A map UID we had no MX ID for just got one: it's now worth keeping much longer. The
        // default implementation would keep what's left of its previous, shorter expiration.
        self.expire_after_create(map_uid, mx_id, updated_at)
    }
}

fn new_cache() -> MxIdCache {
    Cache::builder()
        .max_capacity(MAX_CACHED_MAP_UIDS)
        .expire_after(MxIdExpiry)
        .build()
}

/// Gives the MX ID of the maps, identified by their UID.
///
/// It answers with what we already know, and hands the map UIDs it knows nothing about to a
/// background worker, which groups them to send at most one request to the MX API every five
/// seconds. It never waits for that request: those map UIDs are missing from the answer, and the
/// next calls have them.
///
/// Cloning it is cheap, and gives a handle to the same cache and the same worker.
#[derive(Clone)]
pub struct MxIdProvider {
    cache: MxIdCache,
    batcher: Batcher,
}

impl MxIdProvider {
    /// Creates the provider, and spawns the worker batching the requests to the MX API.
    ///
    /// This must be called from within a Tokio runtime.
    #[inline]
    pub fn from_client<S: MxIdSink>(client: reqwest::Client, sink: S) -> Self {
        Self::spawn(ReqwestMxFetcher::new(client), sink)
    }

    /// Creates the provider on top of the provided fetcher and sink.
    ///
    /// This must be called from within a Tokio runtime.
    pub fn spawn<F: MxFetcher, S: MxIdSink>(fetcher: F, sink: S) -> Self {
        let cache = new_cache();
        let batcher = Batcher::spawn(fetcher, sink, cache.clone());
        Self { cache, batcher }
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
    pub async fn get_mx_ids_of_map_uids(&self, map_uids: &[&str]) -> HashMap<String, i32> {
        let mut ret = HashMap::new();
        let mut map_uids_to_fetch = Vec::new();

        for &map_uid in map_uids {
            match self.cache.get(map_uid).await {
                // We already have its MX ID.
                Some(Some(mx_id)) => {
                    ret.insert(map_uid.to_owned(), mx_id);
                }
                // MX recently told us it doesn't know this map: asking again would be pointless.
                Some(None) => {}
                // Either we never saw it, or that answer expired and is worth asking again.
                None => map_uids_to_fetch.push(map_uid),
            }
        }

        if !map_uids_to_fetch.is_empty() {
            self.batcher.schedule(&map_uids_to_fetch).await;
        }

        ret
    }

    /// Tells what we know about the MX ID of a map, identified by its UID.
    ///
    /// This asks MX nothing, and schedules nothing: it only reports what we have. It's meant for
    /// telling apart "MX doesn't have this map" from "we haven't got an answer yet", which look
    /// the same in [`get_mx_ids`](Self::get_mx_ids_of_map_uids).
    pub async fn status_of(&self, map_uid: &str) -> MxIdStatus {
        match self.cache.get(map_uid).await {
            Some(Some(mx_id)) => MxIdStatus::Known(mx_id),
            Some(None) => MxIdStatus::NotOnMx,
            None => MxIdStatus::Unknown,
        }
    }
}
