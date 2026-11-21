//! The batching worker, one per [`MxQuery`].
//!
//! Nobody talks to MX directly: the keys go through an [`mpsc`] channel to a single worker, which
//! groups them and sends at most one request every [`Policy::flush_interval`].
//!
//! The worker takes three kinds of job, which differ only in who waits for the answer:
//!
//! * [`schedule`](Batcher::schedule) is fire and forget: the answer is written to the cache and to
//!   the [`Sink`], and the callers pick it up on their next call;
//! * [`fetch`](Batcher::fetch) rides along with the same batch, and gets the answer back;
//! * [`force`](Batcher::force) goes straight to MX without waiting for the window, and without
//!   spending it either: it's meant to be triggered by a person, not by our own traffic.
//!
//! The worker is the only owner of that rhythm, and the only thing which talks to MX. It never
//! waits for MX either: a batch is sent by a task of its own, so the worker keeps taking the keys
//! which come in while MX is thinking.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use records_lib::error::{RecordsError, RecordsResult};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, MissedTickBehavior},
};

use super::{Fetcher, MxQuery, Policy, Sink, ValueCache};

/// What MX answered about the keys of one job.
type Answer<Q> = RecordsResult<HashMap<<Q as MxQuery>::Key, <Q as MxQuery>::Value>>;

/// Somebody waiting for the batch their keys left with.
struct Waiter<Q: MxQuery> {
    /// What this one asked for, which is only a part of the batch.
    keys: Vec<Q::Key>,
    reply_tx: oneshot::Sender<Answer<Q>>,
}

/// What the worker is asked to do.
enum Job<Q: MxQuery> {
    /// Fetch these keys whenever the next batch leaves. Nobody is waiting.
    Schedule(Vec<Q::Key>),
    /// Fetch these keys with the next batch, and send the answer back: somebody is waiting for it.
    Fetch(Waiter<Q>),
    /// Fetch these keys right now, off the window, and send the answer back.
    Force(Waiter<Q>),
}

/// What the worker collected during one window.
struct Batch<Q: MxQuery> {
    /// Every key asked for during the window, deduplicated.
    keys: HashSet<Q::Key>,
    /// Whoever asked for a part of them and is waiting for the answer.
    waiters: Vec<Waiter<Q>>,
}

impl<Q: MxQuery> Default for Batch<Q> {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            waiters: Vec::new(),
        }
    }
}

/// Everything a batch needs once it left the worker.
struct Sending<Q: MxQuery, F, S> {
    fetcher: F,
    sink: S,
    cache: ValueCache<Q>,
}

impl<Q: MxQuery, F: Fetcher<Q>, S: Sink<Q>> Sending<Q, F, S> {
    /// Asks MX about the keys, and writes what it answers to the cache.
    ///
    /// The cache comes first: the sooner it holds the answer, the fewer callers ask for it again.
    /// A key missing from the results is one MX doesn't have, and is remembered as such.
    async fn fetch(&self, keys: &HashSet<Q::Key>) -> Answer<Q> {
        let results = {
            let keys = keys.iter().collect::<Vec<_>>();
            self.fetcher.fetch(&keys).await?
        };

        for key in keys {
            let value = results.get(key).cloned();
            self.cache.insert(key.clone(), value).await;
        }

        Ok(results)
    }

    /// Saves what MX answered, and logs whatever goes wrong: nobody waits for this.
    async fn store(&self, results: &HashMap<Q::Key, Q::Value>) {
        if results.is_empty() {
            return;
        }

        if let Err(e) = self.sink.store(results).await {
            tracing::error!("couldn't save {} {} from MX: {e}", results.len(), Q::NAME);
        }
    }
}

/// Picks out of `results` what this waiter asked for.
fn answer_of<Q: MxQuery>(
    waiter: &Waiter<Q>,
    results: &HashMap<Q::Key, Q::Value>,
) -> HashMap<Q::Key, Q::Value> {
    waiter
        .keys
        .iter()
        .filter_map(|key| Some((key.clone(), results.get(key)?.clone())))
        .collect()
}

/// Sends one batch to MX, and hands its answer to the cache, then to whoever waits, then to the
/// sink.
///
/// This runs in a task of its own, so that the worker stays free to take the next jobs while MX is
/// thinking.
async fn send_batch<Q: MxQuery, F: Fetcher<Q>, S: Sink<Q>>(
    sending: Arc<Sending<Q, F, S>>,
    batch: Batch<Q>,
) {
    let results = match sending.fetch(&batch.keys).await {
        Ok(results) => results,
        Err(e) => {
            tracing::error!(
                "couldn't fetch {} {} from MX: {e}",
                batch.keys.len(),
                Q::NAME
            );
            // Several waiters may share this failure, and an error isn't clonable: they get its
            // message, which is what they'd report anyway. Nothing is written down, so these keys
            // are asked again by the next call.
            for waiter in batch.waiters {
                let _ = waiter.reply_tx.send(Err(RecordsError::Internal(format!(
                    "MX request failed: {e}"
                ))));
            }
            return;
        }
    };

    for waiter in batch.waiters {
        let answer = answer_of(&waiter, &results);
        let _ = waiter.reply_tx.send(Ok(answer));
    }

    sending.store(&results).await;
}

/// Asks MX about the keys of one waiter right now, off the window.
///
/// Like [`send_batch`], this runs in a task of its own, so that the worker keeps taking keys while
/// MX is thinking. Being the only one waiting, this one gets the real error.
async fn force_fetch<Q: MxQuery, F: Fetcher<Q>, S: Sink<Q>>(
    sending: Arc<Sending<Q, F, S>>,
    waiter: Waiter<Q>,
) {
    let keys = waiter.keys.iter().cloned().collect();

    let results = match sending.fetch(&keys).await {
        Ok(results) => results,
        Err(e) => {
            let _ = waiter.reply_tx.send(Err(e));
            return;
        }
    };

    // Same order as a batch: the cache (done by `fetch`), then whoever is waiting, then our
    // database.
    let answer = answer_of(&waiter, &results);
    let _ = waiter.reply_tx.send(Ok(answer));

    sending.store(&results).await;
}

/// Takes one job into the batch being collected, and tells whether it was our own traffic.
///
/// A [forced](Job::Force) job is neither batched nor answered here: it leaves on its own, and it
/// doesn't count as traffic, so it never makes a batch leave early.
fn take_job<Q: MxQuery, F: Fetcher<Q>, S: Sink<Q>>(
    sending: &Arc<Sending<Q, F, S>>,
    batch: &mut Batch<Q>,
    job: Job<Q>,
) -> bool {
    match job {
        Job::Schedule(keys) => {
            batch.keys.extend(keys);
            true
        }
        // Somebody is waiting for these, but they're our own traffic like any other: they leave
        // with the next batch, and they open the window.
        Job::Fetch(waiter) => {
            batch.keys.extend(waiter.keys.iter().cloned());
            batch.waiters.push(waiter);
            true
        }
        // A person is asking us to look now, so this doesn't wait for our window. It doesn't spend
        // it either: it isn't our traffic.
        Job::Force(waiter) => {
            tokio::task::spawn(force_fetch(Arc::clone(sending), waiter));
            false
        }
    }
}

/// The worker grouping the requests to MX. See the [module documentation](self).
async fn run_worker<Q: MxQuery, F: Fetcher<Q>, S: Sink<Q>>(
    fetcher: F,
    sink: S,
    cache: ValueCache<Q>,
    mut jobs: mpsc::Receiver<Job<Q>>,
) {
    let sending = Arc::new(Sending {
        fetcher,
        sink,
        cache,
    });

    let Policy { flush_interval, .. } = Q::POLICY;

    let mut interval = tokio::time::interval(flush_interval);
    // We reset the interval after each batch, so a tick we didn't have time to take is one we
    // don't want to take right after.
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // The first tick completes immediately
    interval.tick().await;

    let mut last_fetch_at = None::<Instant>;

    loop {
        let mut batch = Batch::<Q>::default();
        let mut closed = false;
        let mut leave_now = false;

        loop {
            tokio::select! {
                biased;
                job = jobs.recv() => {
                    let Some(job) = job else {
                        closed = true;
                        break;
                    };

                    // Nothing was sent to MX for a while: no reason to hold this batch.
                    if take_job(&sending, &mut batch, job)
                        && last_fetch_at.is_none_or(|at| at.elapsed() >= flush_interval)
                    {
                        leave_now = true;
                        break;
                    }
                }
                // The window is over: what we collected leaves.
                _ = interval.tick() => break,
            }
        }

        // Whatever is already queued leaves with this batch: it arrived before we decided not to
        // wait, and a batch of its own would only cost MX a second request for the same moment.
        if leave_now {
            while let Ok(job) = jobs.try_recv() {
                take_job(&sending, &mut batch, job);
            }
        }

        if !batch.keys.is_empty() {
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

/// A handle to the batching worker of one query.
pub(super) struct Batcher<Q: MxQuery> {
    tx: mpsc::Sender<Job<Q>>,
}

// Derived, this would ask for `Q: Clone`, which a marker type has no reason to be.
impl<Q: MxQuery> Clone for Batcher<Q> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<Q: MxQuery> Batcher<Q> {
    /// Spawns the batching worker, which writes the answers of MX into `cache`, then saves them
    /// through `sink`.
    ///
    /// This must be called from within a Tokio runtime.
    pub(super) fn spawn<F: Fetcher<Q>, S: Sink<Q>>(
        fetcher: F,
        sink: S,
        cache: ValueCache<Q>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(Q::POLICY.queue_size);
        tokio::task::spawn(run_worker(fetcher, sink, cache, rx));
        Self { tx }
    }

    /// Hands the provided keys to the worker, and forgets about them.
    ///
    /// This never waits for MX: the answer goes to the cache and the sink, for the next calls to
    /// pick up. The worker never blocks either, so submitting is a matter of microseconds, even
    /// when its queue is full.
    pub(super) async fn schedule(&self, keys: Vec<Q::Key>) {
        if self.tx.send(Job::Schedule(keys)).await.is_err() {
            tracing::error!("the MX batching worker of the {} is gone", Q::NAME);
        }
    }

    /// Hands the provided keys to the worker and waits for the batch they leave with.
    ///
    /// They spend the window like any other traffic of ours: this waits at most one
    /// [`flush_interval`](Policy::flush_interval) more than the request to MX itself.
    pub(super) async fn fetch(&self, keys: Vec<Q::Key>) -> Answer<Q> {
        self.submit(keys, Job::Fetch).await
    }

    /// Asks MX about the provided keys right now, and waits for its answer.
    ///
    /// This skips the batching window entirely, so it costs a request to MX every time. It's meant
    /// for someone explicitly asking us to look, never for our own traffic.
    pub(super) async fn force(&self, keys: Vec<Q::Key>) -> Answer<Q> {
        self.submit(keys, Job::Force).await
    }

    /// Sends a job somebody waits for, and waits for it.
    async fn submit(&self, keys: Vec<Q::Key>, job: impl FnOnce(Waiter<Q>) -> Job<Q>) -> Answer<Q> {
        let (reply_tx, reply_rx) = oneshot::channel();

        if self.tx.send(job(Waiter { keys, reply_tx })).await.is_err() {
            return Err(RecordsError::Internal(format!(
                "the MX batching worker of the {} is gone",
                Q::NAME
            )));
        }

        reply_rx.await.unwrap_or_else(|_| {
            Err(RecordsError::Internal(format!(
                "the MX batching worker of the {} dropped a request",
                Q::NAME
            )))
        })
    }
}
