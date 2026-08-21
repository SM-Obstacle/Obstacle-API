use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{array, assert_matches};

use rand::distr::Uniform;
use rand::{Rng, rng};
use records_lib::error::RecordsResult;

use crate::maps::{CACHE_PURGE_INTERVAL, CachedMxMapId, CachedMxMapIds, MxSource, Sealed};

struct EmptySource;
impl Sealed for EmptySource {}
impl MxSource for EmptySource {
    async fn fetch_mx_ids(&self, _: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        Ok(Default::default())
    }
}

struct MappingSource<const N: usize>([i32; N]);
impl<const N: usize> MappingSource<N> {
    fn from_iter<I: IntoIterator<Item = i32>>(iter: I) -> Self {
        let mut iter = iter.into_iter();
        Self(array::from_fn(|_| iter.next().unwrap_or_default()))
    }
}
impl<const N: usize> Sealed for MappingSource<N> {}
impl<const N: usize> MxSource for MappingSource<N> {
    async fn fetch_mx_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        Ok(map_uids
            .iter()
            .zip(self.0.iter())
            .map(|(map_uid, mx_id)| (map_uid.to_string(), *mx_id))
            .collect())
    }
}

struct PersistantSource<S> {
    source: S,
    history: Arc<Mutex<Vec<Vec<String>>>>,
}
impl<S> Sealed for PersistantSource<S> {}
impl<S: MxSource> MxSource for PersistantSource<S> {
    async fn fetch_mx_ids(&self, map_uids: &[&str]) -> RecordsResult<HashMap<String, i32>> {
        self.history
            .lock()
            .unwrap()
            .push(map_uids.iter().copied().map(str::to_owned).collect());
        self.source.fetch_mx_ids(map_uids).await
    }
}

#[tokio::test(start_paused = true)]
async fn fetch_once() -> anyhow::Result<()> {
    let history = Arc::new(Mutex::new(Vec::new()));
    let cache = CachedMxMapIds::from_source(PersistantSource {
        source: EmptySource,
        history: Arc::clone(&history),
    });
    cache.get_map_ids(&["foo", "bar"]).await?;
    assert_eq!(*history.lock().unwrap(), [["foo", "bar"]]);
    cache.get_map_ids(&["foo", "bar", "baz"]).await?;
    assert_eq!(
        *history.lock().unwrap(),
        [&["foo", "bar"] as &[&str], &["baz"]]
    );
    cache.get_map_ids(&["bar", "baz"]).await?;
    assert_eq!(
        *history.lock().unwrap(),
        [&["foo", "bar"] as &[&str], &["baz"], &[]]
    );

    Ok(())
}

#[tokio::test]
async fn cached_with_none() -> anyhow::Result<()> {
    let cache = CachedMxMapIds::from_source(EmptySource);

    let returned_ids = cache.get_map_ids(&["foo", "bar"]).await?;
    assert!(returned_ids.is_empty());

    let lock = cache.0.cached.inner.lock().await;
    assert_matches!(
        lock.get("foo"),
        Some(CachedMxMapId {
            map_mx_id: None,
            ..
        })
    );
    assert_matches!(
        lock.get("bar"),
        Some(CachedMxMapId {
            map_mx_id: None,
            ..
        })
    );

    Ok(())
}

#[tokio::test]
async fn cached_with_some() -> anyhow::Result<()> {
    let src @ MappingSource(ids) =
        <MappingSource<2>>::from_iter(rng().sample_iter(Uniform::new(0, u16::MAX as i32).unwrap()));
    let cache = CachedMxMapIds::from_source(src);

    let returned_ids = cache.get_map_ids(&["foo", "bar"]).await?;
    assert_matches!(returned_ids.get("foo"), Some(x) if *x == ids[0]);
    assert_matches!(returned_ids.get("bar"), Some(x) if *x == ids[1]);

    let lock = cache.0.cached.inner.lock().await;
    assert_matches!(
        lock.get("foo"),
        Some(CachedMxMapId {
            map_mx_id: Some(mx_id),
            ..
        }) if *mx_id == returned_ids["foo"]
    );
    assert_matches!(
        lock.get("bar"),
        Some(CachedMxMapId {
            map_mx_id: Some(mx_id),
            ..
        }) if *mx_id == returned_ids["bar"]
    );
    Ok(())
}

#[tokio::test]
async fn cached_with_some_and_none() -> anyhow::Result<()> {
    let src @ MappingSource(ids) =
        <MappingSource<2>>::from_iter(rng().sample_iter(Uniform::new(0, u16::MAX as i32).unwrap()));
    let cache = CachedMxMapIds::from_source(src);

    let returned_ids = cache.get_map_ids(&["foo", "bar", "baz"]).await?;
    assert_matches!(returned_ids.get("foo"), Some(x) if *x == ids[0]);
    assert_matches!(returned_ids.get("bar"), Some(x) if *x == ids[1]);
    assert_matches!(returned_ids.get("baz"), None);

    let lock = cache.0.cached.inner.lock().await;
    assert_matches!(
        lock.get("foo"),
        Some(CachedMxMapId {
            map_mx_id: Some(mx_id),
            ..
        }) if *mx_id == returned_ids["foo"]
    );
    assert_matches!(
        lock.get("bar"),
        Some(CachedMxMapId {
            map_mx_id: Some(mx_id),
            ..
        }) if *mx_id == returned_ids["bar"]
    );
    assert_matches!(
        lock.get("baz"),
        Some(CachedMxMapId {
            map_mx_id: None,
            ..
        })
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn purge_cache() -> anyhow::Result<()> {
    let (cache, notify) = CachedMxMapIds::from_source_with_notify(EmptySource);
    cache.get_map_ids(&["foo", "bar"]).await?;
    {
        let lock = cache.0.cached.inner.lock().await;
        assert!(!lock.is_empty());
    }
    let advance = CACHE_PURGE_INTERVAL + Duration::from_secs(5);
    tokio::time::advance(advance).await;
    tokio::time::timeout(advance + Duration::from_secs(1), notify.notified())
        .await
        .unwrap();
    let lock = cache.0.cached.inner.lock().await;
    assert!(lock.is_empty());

    Ok(())
}
