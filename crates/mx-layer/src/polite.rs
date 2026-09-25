//! How this crate asks [ManiaExchange] anything, whatever it asks for.
//!
//! [ManiaExchange]: https://sm.mania.exchange
//!
//! The MX API can be slow, down, and shouldn't be flooded. Therefore, we implement three
//! principles:
//!
//! **MX is asked at most once per [window](Policy::flush_interval), whatever our own traffic is.**
//! A single worker owns that rhythm, and groups every query during a window into one batch.
//!
//! **An answer of MX is remembered**, so we don't ask twice: what MX gave us for a key, and the
//! keys MX told us it doesn't have. The two don't age the same way, which the cache turns into a
//! per-entry expiration. See [`Policy::known_timeout`] and [`Policy::unknown_timeout`] for more
//! information.
//!
//! **Nobody has to wait for MX.** [`Provider::get_or_schedule`] answers with what we already know
//! and hands the rest to the worker, so a key missing from its answer means "we have no value for
//! it *right now*", not "MX doesn't have it".
//!
//! # The flow
#![doc = simple_mermaid::mermaid!("../docs/flow.mmd")]
//! Two details of that flow are worth spelling out, because they're what keeps everybody free:
//!
//! * the worker **spawns** the batch instead of awaiting it, so it keeps taking keys while MX is
//!   thinking, instead of making the next callers queue behind a request that isn't theirs;
//! * the fetch task writes the cache **before** sending them to the [`Sink`], so that nobody waits
//!   for the `UPDATE`.
//!
//! When a batch fails, nothing is written down at all. The keys are simply asked again by the next
//! call.
//!
//! # Telling "MX doesn't have it" from "not asked yet"
//!
//! In order to distinguish between "MX doesn't have the MX ID" and "we didn't ask yet",
//! [`Provider::status_of`] is used to report what we have cached, without asking MX and without
//! scheduling anything:
#![doc = simple_mermaid::mermaid!("../docs/status.mmd")]

use core::hash::Hash;
use std::{
    borrow::Borrow,
    collections::HashMap,
    future,
    marker::PhantomData,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{FutureExt, StreamExt, stream};
use moka::{Expiry, future::Cache};
use records_lib::error::RecordsResult;

mod batch;

#[cfg(test)]
mod tests;

use batch::Batcher;

/// How politely a [`MxQuery`] is asked.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// The minimum delay between two requests sent to MX for this query.
    pub flush_interval: Duration,
    /// The delay after which we drop a value MX gave us.
    pub known_timeout: Duration,
    /// The delay after which we *might* ask MX again about a key it said it doesn't have.
    pub unknown_timeout: Duration,
    /// The amount of keys kept in memory. The least recently used ones are dropped past that.
    pub max_cached_keys: u64,
    /// The amount of query the batch worker can have in its queue, before the clients (API
    /// endpoints, etc) start to wait. This is only a burst buffer.
    pub queue_size: usize,
}

/// Something we ask MX about, by key. Only used as a type.
pub trait MxQuery: Send + Sync + 'static {
    /// What identifies the query we ask about: a map UID, a mappack ID, etc.
    type Key: Clone + Eq + Hash + Send + Sync + 'static;

    /// What MX answers for one key: an MX ID, an MX map, etc.
    type Value: Clone + Send + Sync + 'static;

    /// How this query is named in the logs, as a plural: `"map MX IDs"`, `"mappack tracks"`, etc.
    const NAME: &'static str;

    /// How politely MX is asked for this query. See [`Policy`].
    const POLICY: Policy;
}

/// A way of asking MX about some keys.
///
/// This is what will directly hit the MX API: it's given the keys of one batch, and calls MX
/// however that endpoint wants to be hit (one request per key, batch requests, etc).
pub trait Fetcher<Q: MxQuery>: Send + Sync + 'static {
    /// Asks MX about the provided keys.
    ///
    /// The returned map only holds the keys MX knows about, so it may be smaller than the provided
    /// slice. The ones it leaves out are the ones MX doesn't have.
    fn fetch<'a>(
        &'a self,
        keys: &'a [&'a Q::Key],
    ) -> impl Future<Output = RecordsResult<HashMap<Q::Key, Q::Value>>> + Send + 'a;
}

/// Where the answers of MX are saved, for the queries which have somewhere to save them.
///
/// In the Obstacle API, this is the trait representing how an MX ID ends up in the `mx_id` column
/// of our maps. This prevents this crate from having to know anything about our database.
pub trait Sink<Q: MxQuery>: Send + Sync + 'static {
    /// Saves what MX answered.
    fn store<'a>(
        &'a self,
        values: &'a HashMap<Q::Key, Q::Value>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a;
}

/// A sink which doesn't save anything, for the queries whose answers are only needed in memory.
impl<Q: MxQuery> Sink<Q> for () {
    #[inline]
    fn store<'a>(
        &'a self,
        _: &'a HashMap<Q::Key, Q::Value>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        std::future::ready(Ok(()))
    }
}

impl<S, Q> Sink<Q> for Arc<S>
where
    S: Sink<Q>,
    Q: MxQuery,
{
    #[inline]
    fn store<'a>(
        &'a self,
        values: &'a HashMap<<Q as MxQuery>::Key, <Q as MxQuery>::Value>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        (**self).store(values)
    }
}

/// What we know about a key, without asking MX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MxStatus<V> {
    /// MX gave us this.
    Known(V),
    /// MX told us it doesn't have this key, recently enough for us to still believe it.
    NotOnMx,
    /// We have no answer for this key: either we never asked, or the last one expired. A batch may
    /// be on its way.
    Unknown,
}

/// What MX told us about the keys: their value, or [`None`] for the ones MX doesn't have.
///
/// The batching worker is its only writer; the [`Provider`]s only read it.
type ValueCache<Q> = Cache<<Q as MxQuery>::Key, Option<<Q as MxQuery>::Value>>;

/// How long an answer of MX is worth keeping, out of [`Q::POLICY`](MxQuery::POLICY).
struct PolicyExpiry<Q>(PhantomData<fn() -> Q>);

impl<Q> PolicyExpiry<Q> {
    #[inline]
    const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Q: MxQuery> Expiry<Q::Key, Option<Q::Value>> for PolicyExpiry<Q> {
    fn expire_after_create(
        &self,
        _: &Q::Key,
        value: &Option<Q::Value>,
        _: Instant,
    ) -> Option<Duration> {
        Some(match value {
            Some(_) => Q::POLICY.known_timeout,
            None => Q::POLICY.unknown_timeout,
        })
    }

    fn expire_after_update(
        &self,
        key: &Q::Key,
        value: &Option<Q::Value>,
        updated_at: Instant,
        _: Option<Duration>,
    ) -> Option<Duration> {
        // A key we had no value for just got one: it's now worth keeping for the other duration.
        // The default implementation would keep what's left of its previous expiration.
        self.expire_after_create(key, value, updated_at)
    }
}

fn new_cache<Q: MxQuery>() -> ValueCache<Q> {
    Cache::builder()
        .max_capacity(Q::POLICY.max_cached_keys)
        .expire_after(PolicyExpiry::<Q>::new())
        .build()
}

/// What the cache has to say about some keys.
struct Split<Q: MxQuery> {
    /// The keys we have a value for, with it.
    known: HashMap<Q::Key, Q::Value>,
    /// The keys MX recently told us it doesn't have.
    absent: Vec<Q::Key>,
    /// The keys we have no answer for at all.
    unanswered: Vec<Q::Key>,
}

/// Answers about one [`MxQuery`], as politely as its [`Policy`] demands.
///
/// It answers with what we already know, and leaves everything else to a single background worker
/// which groups the keys to send at most one request to MX per window. Its callers choose whether
/// they wait for that worker, via [`get_or_fetch`](Self::get_or_fetch).
///
/// Cloning it is cheap, and gives a handle to the same cache and the same worker.
pub struct Provider<Q: MxQuery> {
    cache: ValueCache<Q>,
    batcher: Batcher<Q>,
}

impl<Q: MxQuery> Clone for Provider<Q> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            batcher: self.batcher.clone(),
        }
    }
}

impl<Q: MxQuery> Provider<Q> {
    /// Creates the provider, and spawns the worker batching the requests to MX.
    ///
    /// This must be called from within a Tokio runtime.
    pub fn spawn<F: Fetcher<Q>, S: Sink<Q>>(fetcher: F, sink: S) -> Self {
        let cache = new_cache::<Q>();
        let batcher = Batcher::spawn(fetcher, sink, cache.clone());
        Self { cache, batcher }
    }

    /// Sorts the provided keys into what we have, what MX doesn't have, and what we must ask for.
    async fn split<K>(&self, keys: &[&K]) -> Split<Q>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ToOwned<Owned = Q::Key> + ?Sized + Sync,
    {
        records_lib::assert_future_send(
            stream::iter(keys)
                .map(|key| self.cache.get(*key).map(|value| (*key, value)))
                .buffer_unordered(10)
                .fold(
                    Split::<Q> {
                        known: HashMap::new(),
                        absent: Vec::new(),
                        unanswered: Vec::new(),
                    },
                    |mut split, (key, val)| {
                        match val {
                            // We already have its value.
                            Some(Some(value)) => {
                                split.known.insert(key.to_owned(), value);
                            }
                            // MX recently told us it doesn't have it: asking again would be pointless.
                            Some(None) => split.absent.push(key.to_owned()),
                            // Either we never saw it, or that answer expired and is worth asking again.
                            None => split.unanswered.push(key.to_owned()),
                        }
                        future::ready(split)
                    },
                ),
        )
        .await
    }

    /// Returns what we know about the provided keys, and hands the rest to the worker.
    ///
    /// This never waits for MX. The keys we know nothing about are fetched in the background and
    /// saved, so they're missing from the returned map even though MX may have them. Calling this
    /// again a few seconds later gives them.
    ///
    /// A key missing from the returned map is therefore to be read as "we have no value for it
    /// *right now*".
    pub async fn get_or_schedule<K>(&self, keys: &[&K]) -> HashMap<Q::Key, Q::Value>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ToOwned<Owned = Q::Key> + ?Sized + Sync,
    {
        let split = self.split(keys).await;

        if !split.unanswered.is_empty() {
            self.batcher.schedule(split.unanswered).await;
        }

        split.known
    }

    /// Returns what MX has for the provided keys, waiting for it if we don't have it yet.
    ///
    /// Unlike [`get_or_schedule`](Self::get_or_schedule), this doesn't come back until the answer
    /// is there, for the callers which can't be told to come back later. It's still batched: the
    /// keys join the next window like any other, so several callers asking at the same moment cost
    /// MX a single request.
    ///
    /// The returned map leaves out the keys MX doesn't have.
    pub async fn get_or_fetch<K>(&self, keys: &[&K]) -> RecordsResult<HashMap<Q::Key, Q::Value>>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ToOwned<Owned = Q::Key> + ?Sized + Sync,
    {
        let mut split = self.split(keys).await;

        if !split.unanswered.is_empty() {
            split
                .known
                .extend(self.batcher.fetch(split.unanswered).await?);
        }

        Ok(split.known)
    }

    /// Asks MX about the provided keys right now, and saves what it answers.
    ///
    /// Unlike the two above, this waits for MX, skips the batching window, and ignores a cached
    /// "MX doesn't have it": it's meant for someone who explicitly asks us to look again. It
    /// therefore costs a request to MX every time, and must not be driven by our own traffic.
    ///
    /// The keys we already have a value for need no request, and are answered from the cache.
    pub async fn force<K>(&self, keys: &[&K]) -> RecordsResult<HashMap<Q::Key, Q::Value>>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ToOwned<Owned = Q::Key> + ?Sized + Sync,
    {
        let mut split = self.split(keys).await;

        // Looking again at what MX said it doesn't have is the whole point of this method.
        split.unanswered.append(&mut split.absent);

        if !split.unanswered.is_empty() {
            split
                .known
                .extend(self.batcher.force(split.unanswered).await?);
        }

        Ok(split.known)
    }

    /// Tells what we know about a key.
    ///
    /// This asks MX nothing, and schedules nothing: it only reports what we have. It's meant for
    /// telling apart "MX doesn't have it" from "we haven't got an answer yet", which look the same
    /// in [`get_or_schedule`](Self::get_or_schedule).
    pub async fn status_of<K>(&self, key: &K) -> MxStatus<Q::Value>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ?Sized,
    {
        match self.cache.get(key).await {
            Some(Some(value)) => MxStatus::Known(value),
            Some(None) => MxStatus::NotOnMx,
            None => MxStatus::Unknown,
        }
    }
}

#[cfg(test)]
impl<Q: MxQuery> Provider<Q> {
    /// What the cache holds for this key: nothing, a value, or "MX doesn't have it".
    pub(crate) async fn cached<K>(&self, key: &K) -> Option<Option<Q::Value>>
    where
        Q::Key: Borrow<K>,
        K: Hash + Eq + ?Sized,
    {
        self.cache.get(key).await
    }
}
