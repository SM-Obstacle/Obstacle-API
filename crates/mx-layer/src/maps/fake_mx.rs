//! Fakes used by the tests of this module, to avoid hitting the real MX API and our database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use records_lib::error::{RecordsError, RecordsResult};

use super::{MxFetcher, MxIdSink};

/// Builds a hash map of MX IDs, to compare with what the provider returns.
pub(super) fn ids<const N: usize>(ids: [(&str, i32); N]) -> HashMap<String, i32> {
    ids.into_iter()
        .map(|(map_uid, mx_id)| (map_uid.to_owned(), mx_id))
        .collect()
}

/// Lets the spawned tasks make progress, without letting the paused clock auto-advance.
pub(super) async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

#[derive(Default)]
struct FakeMxInner {
    /// The map UIDs this fake MX knows about.
    known: HashMap<String, i32>,
    /// The batches received so far, each one with its map UIDs sorted.
    calls: Mutex<Vec<Vec<String>>>,
    /// How long a batch takes to be answered.
    latency: Duration,
    /// Whether every batch must fail.
    failing: bool,
}

/// A fake [`MxFetcher`] which records every batch it receives.
#[derive(Clone, Default)]
pub(super) struct FakeMx(Arc<FakeMxInner>);

impl FakeMx {
    /// A fake MX knowing the provided map UIDs, and answering right away.
    pub(super) fn new<const N: usize>(known: [(&str, i32); N]) -> Self {
        Self::with_latency(known, Duration::ZERO)
    }

    /// A fake MX taking `latency` to answer, to observe what happens while a batch is in flight.
    pub(super) fn with_latency<const N: usize>(known: [(&str, i32); N], latency: Duration) -> Self {
        Self(Arc::new(FakeMxInner {
            known: ids(known),
            latency,
            ..Default::default()
        }))
    }

    /// A fake MX failing every batch.
    pub(super) fn failing() -> Self {
        Self(Arc::new(FakeMxInner {
            failing: true,
            ..Default::default()
        }))
    }

    /// The batches received so far, in order, each one with its map UIDs sorted.
    pub(super) fn calls(&self) -> Vec<Vec<String>> {
        self.0.calls.lock().unwrap().clone()
    }

    pub(super) fn call_count(&self) -> usize {
        self.0.calls.lock().unwrap().len()
    }
}

impl MxFetcher for FakeMx {
    #[allow(clippy::manual_async_fn)]
    fn fetch_mx_ids<'a>(
        &'a self,
        map_uids: &'a [&'a str],
    ) -> impl Future<Output = RecordsResult<HashMap<String, i32>>> + Send + 'a {
        async move {
            let mut batch = map_uids
                .iter()
                .copied()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            // A batch is built from a set, so its order isn't deterministic.
            batch.sort();
            self.0.calls.lock().unwrap().push(batch);

            if !self.0.latency.is_zero() {
                tokio::time::sleep(self.0.latency).await;
            }

            if self.0.failing {
                return Err(RecordsError::Internal("MX is down".to_owned()));
            }

            Ok(map_uids
                .iter()
                .filter_map(|map_uid| Some(((*map_uid).to_owned(), *self.0.known.get(*map_uid)?)))
                .collect())
        }
    }
}

/// A fake [`MxIdSink`] which records what it was asked to save.
#[derive(Clone, Default)]
pub(super) struct FakeSink(Arc<Mutex<Vec<HashMap<String, i32>>>>);

impl FakeSink {
    /// Everything it was asked to save, in order.
    pub(super) fn stored(&self) -> Vec<HashMap<String, i32>> {
        self.0.lock().unwrap().clone()
    }
}

impl MxIdSink for FakeSink {
    #[allow(clippy::manual_async_fn)]
    fn store<'a>(
        &'a self,
        map_ids: &'a HashMap<String, i32>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        async move {
            self.0.lock().unwrap().push(map_ids.clone());
            Ok(())
        }
    }
}
