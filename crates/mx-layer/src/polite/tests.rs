use std::time::Duration;

use moka::Expiry as _;
use tokio::time::Instant;

use crate::fakes::{FakeMx, FakeSink, ids, settle};

use super::{MxQuery, MxStatus, PolicyExpiry, Provider};

/// The query the politeness layer is tested with. Only its policy matters here: what it stands for
/// is the business of the modules which own a real one.
enum TestQuery {}

impl MxQuery for TestQuery {
    type Key = String;
    type Value = i32;

    const NAME: &'static str = "test values";

    const POLICY: super::Policy = super::Policy {
        flush_interval: Duration::from_secs(2),
        known_timeout: Duration::from_hours(2),
        unknown_timeout: Duration::from_hours(24),
        max_cached_keys: 1_000,
        queue_size: 100,
    };
}

const FLUSH_INTERVAL: Duration = TestQuery::POLICY.flush_interval;
const KNOWN_TIMEOUT: Duration = TestQuery::POLICY.known_timeout;
const UNKNOWN_TIMEOUT: Duration = TestQuery::POLICY.unknown_timeout;

fn spawn(mx: &FakeMx) -> Provider<TestQuery> {
    Provider::spawn(mx.clone(), ())
}

fn spawn_with_sink(mx: &FakeMx, sink: &FakeSink) -> Provider<TestQuery> {
    Provider::spawn(mx.clone(), sink.clone())
}

/// The expiration of the entries is left to moka, which uses its own clock: the tests can't move
/// it, so this is what covers how long an answer of MX lives.
#[test]
fn an_answer_we_got_is_kept_for_another_time_than_an_absence() {
    let key = "foo".to_owned();
    let now = std::time::Instant::now();
    let expiry = PolicyExpiry::<TestQuery>::new();

    assert_eq!(
        expiry.expire_after_create(&key, &Some(42), now),
        Some(KNOWN_TIMEOUT)
    );
    assert_eq!(
        expiry.expire_after_create(&key, &None, now),
        Some(UNKNOWN_TIMEOUT)
    );

    // A key MX didn't have which finally got a value mustn't inherit what was left of its previous
    // expiration.
    assert_eq!(
        expiry.expire_after_update(&key, &Some(42), now, Some(Duration::from_secs(1))),
        Some(KNOWN_TIMEOUT)
    );
}

#[tokio::test(start_paused = true)]
async fn the_first_batch_leaves_right_away() {
    let mx = FakeMx::new([("a", 1)]);
    let sink = FakeSink::default();
    let provider = spawn_with_sink(&mx, &sink);

    let at = Instant::now();
    assert!(provider.get_or_schedule(&["a", "b"]).await.is_empty());

    // Scheduling is fire and forget: the caller doesn't wait for MX.
    assert_eq!(at.elapsed(), Duration::ZERO);
    settle().await;

    assert_eq!(mx.calls(), [["a", "b"]]);
    // MX answered for `a`, and doesn't have `b`: both are worth remembering.
    assert_eq!(provider.cached("a").await, Some(Some(1)));
    assert_eq!(provider.cached("b").await, Some(None));
    // Only the values we actually got are saved.
    assert_eq!(sink.stored(), [ids([("a", 1)])]);
}

#[tokio::test(start_paused = true)]
async fn requests_during_the_window_are_held_and_dont_wait() {
    let mx = FakeMx::new([("a", 1), ("b", 2), ("c", 3)]);
    let provider = spawn(&mx);

    // This one opens the window.
    provider.get_or_schedule(&["a"]).await;
    settle().await;

    provider.get_or_schedule(&["b"]).await;
    provider.get_or_schedule(&["c", "b"]).await;
    settle().await;

    // They're held until the window closes.
    assert_eq!(mx.call_count(), 1);
    assert_eq!(provider.cached("b").await, None);

    // Nobody asks for anything else: the window closing is what sends them.
    tokio::time::advance(FLUSH_INTERVAL).await;
    settle().await;

    // The two requests left as a single batch, with the keys deduplicated.
    assert_eq!(mx.calls(), [vec!["a"], vec!["b", "c"]]);
    assert_eq!(provider.cached("b").await, Some(Some(2)));
    assert_eq!(provider.cached("c").await, Some(Some(3)));
}

#[tokio::test(start_paused = true)]
async fn the_window_is_measured_from_the_last_batch_not_from_the_worker_start() {
    let mx = FakeMx::new([("a", 1), ("b", 2)]);
    let provider = spawn(&mx);

    // Let the worker start, so that its interval is due to fire one window from now...
    settle().await;
    // ...then let half a window pass without anything happening.
    tokio::time::advance(FLUSH_INTERVAL / 2).await;
    settle().await;

    // ...and this batch leaves right away, off that grid.
    provider.get_or_schedule(&["a"]).await;
    settle().await;
    provider.get_or_schedule(&["b"]).await;
    settle().await;

    // The grid would fire here, only half a window after the batch above. It mustn't: the window
    // is measured from the last batch, otherwise two requests leave MX moments apart.
    tokio::time::advance(FLUSH_INTERVAL / 2).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // A whole window after that batch, though, `b` does leave.
    tokio::time::advance(FLUSH_INTERVAL / 2).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"], ["b"]]);
}

#[tokio::test(start_paused = true)]
async fn an_idle_window_doesnt_send_anything() {
    let mx = FakeMx::new([("a", 1)]);
    let provider = spawn(&mx);

    tokio::time::advance(FLUSH_INTERVAL * 10).await;
    settle().await;

    assert_eq!(mx.call_count(), 0);

    // And a key arriving after that quiet period leaves right away, without waiting for the next
    // window.
    provider.get_or_schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"]]);
}

/// A batch which leaves without waiting for the window takes with it whatever is already queued:
/// two callers arriving at the same moment are one request, not two.
#[tokio::test(start_paused = true)]
async fn a_batch_leaving_early_takes_what_is_already_queued() {
    let mx = FakeMx::new([("a", 1), ("b", 2)]);
    let provider = spawn(&mx);
    let other = provider.clone();

    // Neither of them lets the worker run before the other has asked.
    tokio::join!(async { provider.get_or_schedule(&["a"]).await }, async {
        other.get_or_schedule(&["b"]).await
    },);
    settle().await;

    assert_eq!(mx.calls(), [["a", "b"]]);
}

#[tokio::test(start_paused = true)]
async fn a_failed_batch_writes_nothing_down() {
    let mx = FakeMx::failing();
    let sink = FakeSink::default();
    let provider = spawn_with_sink(&mx, &sink);

    provider.get_or_schedule(&["a"]).await;
    settle().await;

    // Nothing is cached, so nothing pretends MX doesn't have it.
    assert_eq!(provider.cached("a").await, None);
    assert!(sink.stored().is_empty());

    // ...and it's asked again at the next window.
    tokio::time::advance(FLUSH_INTERVAL).await;
    provider.get_or_schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.calls(), [["a"], ["a"]]);
}

#[tokio::test(start_paused = true)]
async fn the_worker_stays_free_while_mx_is_thinking() {
    let mx = FakeMx::with_latency([("a", 1), ("b", 2)], FLUSH_INTERVAL * 4);
    let provider = spawn(&mx);

    provider.get_or_schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // The batch is in flight for a long while. The worker must keep taking keys instead of making
    // everybody wait for MX.
    let at = Instant::now();
    provider.get_or_schedule(&["b"]).await;
    assert_eq!(at.elapsed(), Duration::ZERO);
    settle().await;

    tokio::time::advance(FLUSH_INTERVAL * 4).await;
    settle().await;
    assert_eq!(provider.cached("a").await, Some(Some(1)));
}

#[tokio::test(start_paused = true)]
async fn get_or_fetch_waits_for_the_window_instead_of_asking_again() -> anyhow::Result<()> {
    let mx = FakeMx::new([("a", 1), ("b", 2)]);
    let provider = spawn(&mx);

    // Somebody else opens the window.
    provider.get_or_schedule(&["a"]).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // This caller can't be told to come back later, but it doesn't get to jump the queue either:
    // it waits for the window like the rest of our traffic.
    let at = Instant::now();
    assert_eq!(provider.get_or_fetch(&["b"]).await?, ids([("b", 2)]));
    assert_eq!(at.elapsed(), FLUSH_INTERVAL);
    assert_eq!(mx.calls(), [["a"], ["b"]]);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn callers_waiting_for_the_same_key_share_one_request() -> anyhow::Result<()> {
    let mx = FakeMx::new([("a", 1)]);
    let provider = spawn(&mx);
    let other = provider.clone();

    // The window is open, so the two of them are held by it and leave together.
    provider.get_or_schedule(&["z"]).await;
    settle().await;

    let (mine, theirs) = tokio::join!(provider.get_or_fetch(&["a"]), other.get_or_fetch(&["a"]));

    // Both got the answer, and MX was asked once.
    assert_eq!(mine?, ids([("a", 1)]));
    assert_eq!(theirs?, ids([("a", 1)]));
    assert_eq!(mx.calls(), [vec!["z"], vec!["a"]]);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn get_or_fetch_leaves_out_what_mx_doesnt_have() -> anyhow::Result<()> {
    let mx = FakeMx::new([("a", 1)]);
    let provider = spawn(&mx);

    assert_eq!(provider.get_or_fetch(&["a", "b"]).await?, ids([("a", 1)]));
    // And that absence is remembered like any other answer.
    assert_eq!(provider.status_of("b").await, MxStatus::NotOnMx);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn get_or_fetch_doesnt_ask_for_what_we_already_know() -> anyhow::Result<()> {
    let mx = FakeMx::new([("a", 1)]);
    let provider = spawn(&mx);

    provider.get_or_schedule(&["a"]).await;
    settle().await;

    let at = Instant::now();
    assert_eq!(provider.get_or_fetch(&["a"]).await?, ids([("a", 1)]));
    // Answered from the cache: neither a request nor a wait.
    assert_eq!(at.elapsed(), Duration::ZERO);
    assert_eq!(mx.call_count(), 1);

    Ok(())
}

/// A failed batch is nobody's fault in particular, and its error can't be handed to several
/// waiters at once. They must still be told, rather than left hanging until the timeout of
/// whoever is waiting on them.
#[tokio::test(start_paused = true)]
async fn a_failed_batch_is_reported_to_whoever_waits() {
    let mx = FakeMx::failing();
    let provider = spawn(&mx);

    assert!(provider.get_or_fetch(&["a"]).await.is_err());
    // And nothing was written down, so it's asked again rather than read as an absence.
    assert_eq!(provider.status_of("a").await, MxStatus::Unknown);
}
