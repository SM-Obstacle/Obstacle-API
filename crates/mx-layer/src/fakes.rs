//! Fakes used by the tests of this crate, to avoid hitting the real MX API and our database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use records_lib::error::{RecordsError, RecordsResult};

use crate::polite::{Fetcher, MxQuery, Sink};

/// The queries the fakes below can answer for: the ones naming a thing with a string and getting a
/// number back.
///
/// [`MapMxIds`](crate::maps::MapMxIds) is one, and so is the query the politeness layer tests
/// itself with, so the two test suites share these fakes.
pub(crate) trait NumberedByName: MxQuery<Key = String, Value = i32> {}

impl<Q: MxQuery<Key = String, Value = i32>> NumberedByName for Q {}

/// Builds a hash map of values, to compare with what a provider returns.
pub(crate) fn ids<const N: usize>(ids: [(&str, i32); N]) -> HashMap<String, i32> {
    ids.into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// Lets the spawned tasks make progress, without letting the paused clock auto-advance.
pub(crate) async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

#[derive(Default)]
struct FakeMxInner {
    /// The keys this fake MX knows about. Behind a lock, because a map may be uploaded to MX while
    /// a test is running.
    known: Mutex<HashMap<String, i32>>,
    /// The batches received so far, each one with its keys sorted.
    calls: Mutex<Vec<Vec<String>>>,
    /// How long a batch takes to be answered.
    latency: Duration,
    /// Whether every batch must fail.
    failing: bool,
}

/// A fake [`Fetcher`] which records every batch it receives.
#[derive(Clone, Default)]
pub(crate) struct FakeMx(Arc<FakeMxInner>);

impl FakeMx {
    /// A fake MX knowing the provided keys, and answering right away.
    pub(crate) fn new<const N: usize>(known: [(&str, i32); N]) -> Self {
        Self::with_latency(known, Duration::ZERO)
    }

    /// A fake MX taking `latency` to answer, to observe what happens while a batch is in flight.
    pub(crate) fn with_latency<const N: usize>(known: [(&str, i32); N], latency: Duration) -> Self {
        Self(Arc::new(FakeMxInner {
            known: Mutex::new(ids(known)),
            latency,
            ..Default::default()
        }))
    }

    /// A fake MX failing every batch.
    pub(crate) fn failing() -> Self {
        Self(Arc::new(FakeMxInner {
            failing: true,
            ..Default::default()
        }))
    }

    /// Uploads something to this fake MX, which didn't have it until now.
    pub(crate) fn add(&self, key: &str, value: i32) {
        self.0.known.lock().unwrap().insert(key.to_owned(), value);
    }

    /// The batches received so far, in order, each one with its keys sorted.
    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        self.0.calls.lock().unwrap().clone()
    }

    pub(crate) fn call_count(&self) -> usize {
        self.0.calls.lock().unwrap().len()
    }
}

impl<Q: NumberedByName> Fetcher<Q> for FakeMx {
    #[allow(clippy::manual_async_fn)]
    fn fetch<'a>(
        &'a self,
        keys: &'a [&'a String],
    ) -> impl Future<Output = RecordsResult<HashMap<String, i32>>> + Send + 'a {
        async move {
            let mut batch = keys.iter().map(|key| (*key).clone()).collect::<Vec<_>>();
            // A batch is built from a set, so its order isn't deterministic.
            batch.sort();
            self.0.calls.lock().unwrap().push(batch);

            if !self.0.latency.is_zero() {
                tokio::time::sleep(self.0.latency).await;
            }

            if self.0.failing {
                return Err(RecordsError::Internal("MX is down".to_owned()));
            }

            let known = self.0.known.lock().unwrap();

            Ok(keys
                .iter()
                .filter_map(|key| Some(((*key).clone(), *known.get(*key)?)))
                .collect())
        }
    }
}

/// A fake [`Sink`] which records what it was asked to save.
#[derive(Clone, Default)]
pub(crate) struct FakeSink(Arc<Mutex<Vec<HashMap<String, i32>>>>);

impl FakeSink {
    /// Everything it was asked to save, in order.
    pub(crate) fn stored(&self) -> Vec<HashMap<String, i32>> {
        self.0.lock().unwrap().clone()
    }
}

impl<Q: NumberedByName> Sink<Q> for FakeSink {
    #[allow(clippy::manual_async_fn)]
    fn store<'a>(
        &'a self,
        values: &'a HashMap<String, i32>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        async move {
            self.0.lock().unwrap().push(values.clone());
            Ok(())
        }
    }
}
