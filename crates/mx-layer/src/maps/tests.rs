use std::time::Duration;

use tokio::time::Instant;

use moka::Expiry as _;

use super::batch::FLUSH_INTERVAL;
use super::fake_mx::{FakeMx, FakeSink, ids, settle};
use super::{KNOWN_TIMEOUT, MxIdExpiry, MxIdProvider, MxIdStatus, UNKNOWN_TIMEOUT};

impl MxIdProvider {
    /// What the cache holds for this map UID: nothing, an MX ID, or "MX doesn't know it".
    async fn cached(&self, map_uid: &str) -> Option<Option<i32>> {
        self.cache.get(map_uid).await
    }
}

/// The expiration of the entries is left to moka, which uses its own clock: the tests can't move
/// it, so this is what covers how long an answer of MX lives.
#[test]
fn an_mx_id_we_know_is_kept_much_longer_than_one_we_dont() {
    let map_uid = "foo".to_owned();
    let now = std::time::Instant::now();

    assert_eq!(
        MxIdExpiry.expire_after_create(&map_uid, &Some(42), now),
        Some(KNOWN_TIMEOUT)
    );
    assert_eq!(
        MxIdExpiry.expire_after_create(&map_uid, &None, now),
        Some(UNKNOWN_TIMEOUT)
    );

    // A map UID MX didn't know which finally got an MX ID mustn't inherit what was left of its
    // previous, much shorter expiration.
    assert_eq!(
        MxIdExpiry.expire_after_update(&map_uid, &Some(42), now, Some(Duration::from_secs(1))),
        Some(KNOWN_TIMEOUT)
    );
}

#[tokio::test(start_paused = true)]
async fn get_mx_ids_never_waits_for_mx() {
    let mx = FakeMx::with_latency([("foo", 42)], Duration::from_secs(60));
    let provider = MxIdProvider::spawn(mx.clone(), ());

    // The caller is on a hot path: it gets nothing for now, and the map UID is on its way.
    let at = Instant::now();
    assert!(provider.get_mx_ids_of_map_uids(&["foo"]).await.is_empty());
    assert_eq!(at.elapsed(), Duration::ZERO);

    settle().await;
    assert_eq!(mx.calls(), [["foo"]]);
}

#[tokio::test(start_paused = true)]
async fn map_ids_are_there_once_their_batch_came_back() {
    let mx = FakeMx::new([("foo", 42), ("bar", 1337)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());

    assert!(
        provider
            .get_mx_ids_of_map_uids(&["foo", "bar", "baz"])
            .await
            .is_empty()
    );
    settle().await;

    assert_eq!(
        provider
            .get_mx_ids_of_map_uids(&["foo", "bar", "baz"])
            .await,
        ids([("foo", 42), ("bar", 1337)])
    );
    // `baz` isn't on MX, and we don't ask again for it.
    assert_eq!(provider.cached("baz").await, Some(None));
    assert_eq!(mx.calls(), [["bar", "baz", "foo"]]);
}

#[tokio::test(start_paused = true)]
async fn known_map_ids_are_never_asked_again() {
    let mx = FakeMx::new([("foo", 42)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());

    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;

    // An MX ID doesn't change, so it stays good.
    tokio::time::advance(FLUSH_INTERVAL * 10).await;
    assert_eq!(
        provider.get_mx_ids_of_map_uids(&["foo"]).await,
        ids([("foo", 42)])
    );
    settle().await;
    assert_eq!(mx.call_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn unknown_map_uids_arent_asked_again_right_away() {
    let mx = FakeMx::default();
    let provider = MxIdProvider::spawn(mx.clone(), ());

    assert!(provider.get_mx_ids_of_map_uids(&["foo"]).await.is_empty());
    settle().await;
    assert_eq!(provider.cached("foo").await, Some(None));

    // MX just told us it doesn't know this map: asking again would be pointless, until that
    // answer expires (see `an_mx_id_we_know_is_kept_much_longer_than_one_we_dont`).
    tokio::time::advance(FLUSH_INTERVAL * 2).await;
    assert!(provider.get_mx_ids_of_map_uids(&["foo"]).await.is_empty());
    settle().await;
    assert_eq!(mx.call_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn map_uids_of_several_callers_leave_as_one_batch() {
    let mx = FakeMx::new([("foo", 42), ("bar", 1337)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());
    let other = provider.clone();

    // Somebody else opens the window.
    other.get_mx_ids_of_map_uids(&["baz"]).await;
    settle().await;

    other.get_mx_ids_of_map_uids(&["bar"]).await;
    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;
    assert_eq!(mx.call_count(), 1);

    tokio::time::advance(FLUSH_INTERVAL).await;
    settle().await;

    assert_eq!(mx.calls(), [vec!["baz"], vec!["bar", "foo"]]);
    assert_eq!(
        provider.get_mx_ids_of_map_uids(&["foo"]).await,
        ids([("foo", 42)])
    );
    assert_eq!(
        other.get_mx_ids_of_map_uids(&["bar"]).await,
        ids([("bar", 1337)])
    );
}

#[tokio::test(start_paused = true)]
async fn fetched_map_ids_are_stored_through_the_sink() {
    let mx = FakeMx::new([("foo", 42)]);
    let sink = FakeSink::default();
    let provider = MxIdProvider::spawn(mx.clone(), sink.clone());

    provider.get_mx_ids_of_map_uids(&["foo", "bar"]).await;
    settle().await;

    // Only what MX actually knows is saved; `bar` has no MX ID to write down.
    assert_eq!(sink.stored(), [ids([("foo", 42)])]);
}

#[tokio::test(start_paused = true)]
async fn status_tells_a_map_absent_from_mx_from_one_we_havent_asked_about() {
    let mx = FakeMx::new([("foo", 42)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());

    // We have no answer for either of them yet: `mxId` being null means nothing more than that.
    assert_eq!(provider.status_of("foo").await, MxIdStatus::Unknown);
    assert_eq!(provider.status_of("bar").await, MxIdStatus::Unknown);

    provider.get_mx_ids_of_map_uids(&["foo", "bar"]).await;
    settle().await;

    // Now the two are told apart.
    assert_eq!(provider.status_of("foo").await, MxIdStatus::Known(42));
    assert_eq!(provider.status_of("bar").await, MxIdStatus::NotOnMx);

    // And asking doesn't schedule anything: it only reports.
    assert_eq!(mx.call_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn force_fetch_goes_to_mx_right_away() -> anyhow::Result<()> {
    let mx = FakeMx::new([("foo", 42), ("bar", 1337)]);
    let sink = FakeSink::default();
    let provider = MxIdProvider::spawn(mx.clone(), sink.clone());

    // This opens the batching window.
    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;

    // Which the forced fetch doesn't wait for.
    let at = Instant::now();
    assert_eq!(provider.force_fetch("bar").await?, Some(1337));
    assert_eq!(at.elapsed(), Duration::ZERO);

    assert_eq!(mx.calls(), [["foo"], ["bar"]]);
    // And it's written down like any other answer.
    assert_eq!(provider.cached("bar").await, Some(Some(1337)));
    assert_eq!(sink.stored(), [ids([("foo", 42)]), ids([("bar", 1337)])]);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn force_fetch_ignores_a_cached_absence() -> anyhow::Result<()> {
    let mx = FakeMx::default();
    let provider = MxIdProvider::spawn(mx.clone(), ());

    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;
    assert_eq!(provider.cached("foo").await, Some(None));

    // The map lands on MX. We have no way of knowing, and our cached answer says otherwise for
    // the next 24 hours.
    mx.add("foo", 42);
    assert!(provider.get_mx_ids_of_map_uids(&["foo"]).await.is_empty());
    settle().await;
    assert_eq!(mx.call_count(), 1);

    // That's exactly what a forced fetch is for.
    assert_eq!(provider.force_fetch("foo").await?, Some(42));
    assert_eq!(mx.calls(), [["foo"], ["foo"]]);
    assert_eq!(
        provider.get_mx_ids_of_map_uids(&["foo"]).await,
        ids([("foo", 42)])
    );

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn force_fetch_doesnt_ask_for_what_we_already_know() -> anyhow::Result<()> {
    let mx = FakeMx::new([("foo", 42)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());

    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;

    assert_eq!(provider.force_fetch("foo").await?, Some(42));
    assert_eq!(mx.call_count(), 1);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn force_fetch_doesnt_spend_the_batching_window() -> anyhow::Result<()> {
    let mx = FakeMx::new([("foo", 42), ("bar", 1337), ("baz", 7)]);
    let provider = MxIdProvider::spawn(mx.clone(), ());

    // The window opens here, so it closes 5 seconds from now.
    provider.get_mx_ids_of_map_uids(&["foo"]).await;
    settle().await;

    tokio::time::advance(FLUSH_INTERVAL / 2).await;
    provider.force_fetch("bar").await?;
    provider.get_mx_ids_of_map_uids(&["baz"]).await;
    settle().await;
    assert_eq!(mx.call_count(), 2);

    // `baz` must leave one window after `foo`, not after the forced fetch: a person asking us to
    // look isn't our own traffic, so it doesn't push the window.
    tokio::time::advance(FLUSH_INTERVAL / 2).await;
    settle().await;
    assert_eq!(mx.calls(), [["foo"], ["bar"], ["baz"]]);

    Ok(())
}

/// A forced fetch which finds nothing must be remembered like any other answer, otherwise the
/// website keeps offering to look for a map ManiaExchange has just said it doesn't have.
#[tokio::test(start_paused = true)]
async fn a_forced_fetch_that_finds_nothing_is_remembered() -> anyhow::Result<()> {
    let mx = FakeMx::default();
    let provider = MxIdProvider::spawn(mx.clone(), ());

    assert_eq!(provider.force_fetch("foo").await?, None);
    assert_eq!(provider.status_of("foo").await, MxIdStatus::NotOnMx);

    Ok(())
}
