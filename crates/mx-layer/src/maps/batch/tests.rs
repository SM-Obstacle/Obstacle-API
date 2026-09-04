use std::time::Duration;

use tokio::time::Instant;

use super::super::fake_mx::{FakeMx, FakeSink, ids, settle};
use super::super::{MxIdCache, new_cache};
use super::{Batcher, FLUSH_INTERVAL};

fn spawn_worker(mx: &FakeMx) -> (Batcher, MxIdCache, FakeSink) {
    let cache = new_cache();
    let sink = FakeSink::default();
    let batcher = Batcher::spawn(mx.clone(), sink.clone(), cache.clone());
    (batcher, cache, sink)
}

async fn cached(cache: &MxIdCache, map_uid: &str) -> Option<Option<i32>> {
    cache.get(map_uid).await
}

#[tokio::test(start_paused = true)]
async fn the_first_batch_leaves_right_away() {
    let mx = FakeMx::new([("a", 1)]);
    let (batcher, cache, sink) = spawn_worker(&mx);

    let at = Instant::now();
    batcher.schedule(&["a", "b"]).await;

    // Scheduling is fire and forget: the caller doesn't wait for MX.
    assert_eq!(at.elapsed(), Duration::ZERO);
    settle().await;

    assert_eq!(mx.calls(), [["a", "b"]]);
    // MX answered for `a`, and doesn't know `b`: both are worth remembering.
    assert_eq!(cached(&cache, "a").await, Some(Some(1)));
    assert_eq!(cached(&cache, "b").await, Some(None));
    // Only the MX IDs we actually got are saved.
    assert_eq!(sink.stored(), [ids([("a", 1)])]);
}

#[tokio::test(start_paused = true)]
async fn requests_during_the_window_are_held_and_dont_wait() {
    let mx = FakeMx::new([("a", 1), ("b", 2), ("c", 3)]);
    let (batcher, cache, _sink) = spawn_worker(&mx);

    // This one opens the window.
    batcher.schedule(&["a"]).await;
    settle().await;

    batcher.schedule(&["b"]).await;
    batcher.schedule(&["c", "b"]).await;
    settle().await;

    // They're held until the window closes.
    assert_eq!(mx.call_count(), 1);
    assert_eq!(cached(&cache, "b").await, None);

    // Nobody asks for anything else: the window closing is what sends them.
    tokio::time::advance(FLUSH_INTERVAL).await;
    settle().await;

    // The two requests left as a single batch, with the map UIDs deduplicated.
    assert_eq!(mx.calls(), [vec!["a"], vec!["b", "c"]]);
    assert_eq!(cached(&cache, "b").await, Some(Some(2)));
    assert_eq!(cached(&cache, "c").await, Some(Some(3)));
}

#[tokio::test(start_paused = true)]
async fn the_window_is_measured_from_the_last_batch_not_from_the_worker_start() {
    let mx = FakeMx::new([("a", 1), ("b", 2)]);
    let (batcher, _cache, _sink) = spawn_worker(&mx);

    // Let the worker start, so that its interval is due to fire at 5s, 10s...
    settle().await;
    // ...then let two seconds pass without anything happening.
    tokio::time::advance(Duration::from_secs(2)).await;
    settle().await;

    // ...and this batch leaves right away, off that grid.
    batcher.schedule(&["a"]).await;
    settle().await;
    batcher.schedule(&["b"]).await;
    settle().await;

    // The grid would fire here, only 3 seconds after the batch above. It mustn't: the window is
    // measured from the last batch, otherwise two requests leave MX a few milliseconds apart.
    tokio::time::advance(Duration::from_secs(3)).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // 5 seconds after the batch, though, `b` does leave.
    tokio::time::advance(Duration::from_secs(2)).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"], ["b"]]);
}

#[tokio::test(start_paused = true)]
async fn an_idle_window_doesnt_send_anything() {
    let mx = FakeMx::new([("a", 1)]);
    let (batcher, _cache, _sink) = spawn_worker(&mx);

    tokio::time::advance(FLUSH_INTERVAL * 10).await;
    settle().await;

    assert_eq!(mx.call_count(), 0);

    // And a map UID arriving after that quiet period leaves right away, without waiting for the
    // next window.
    batcher.schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"]]);
}

#[tokio::test(start_paused = true)]
async fn a_failed_batch_writes_nothing_down() {
    let mx = FakeMx::failing();
    let (batcher, cache, sink) = spawn_worker(&mx);

    batcher.schedule(&["a"]).await;
    settle().await;

    // Nothing is cached, so nothing pretends MX doesn't know this map.
    assert_eq!(cached(&cache, "a").await, None);
    assert!(sink.stored().is_empty());

    // ...and it's asked again at the next window.
    tokio::time::advance(FLUSH_INTERVAL).await;
    batcher.schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"], ["a"]]);
}

#[tokio::test(start_paused = true)]
async fn the_worker_stays_free_while_mx_is_thinking() {
    let mx = FakeMx::with_latency([("a", 1), ("b", 2)], FLUSH_INTERVAL * 4);
    let (batcher, cache, _sink) = spawn_worker(&mx);

    batcher.schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // The batch is in flight for a long while. The worker must keep taking map UIDs instead of
    // making everybody wait for MX.
    let at = Instant::now();
    batcher.schedule(&["b"]).await;
    assert_eq!(at.elapsed(), Duration::ZERO);
    settle().await;

    tokio::time::advance(FLUSH_INTERVAL * 4).await;
    settle().await;
    assert_eq!(cached(&cache, "a").await, Some(Some(1)));
}
