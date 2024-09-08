use std::{
    collections::HashSet,
    convert::Infallible,
    future::{ready, Ready},
    sync::Arc,
};

use actix_web::FromRequest;
use tokio::sync::RwLock;

#[derive(Default)]
struct Impl {
    data: HashSet<u32>,
}

impl Impl {
    #[inline(always)]
    fn is_finish_pending(&self, map_id: u32) -> bool {
        self.data.contains(&map_id)
    }

    fn pend(&mut self, map_id: u32) {
        self.data.insert(map_id);
    }

    fn release(&mut self, map_id: u32) {
        self.data.remove(&map_id);
    }
}

#[derive(Clone, Default)]
pub struct FinishLocker {
    inner: Arc<RwLock<Impl>>,
}

impl FinishLocker {
    pub async fn wait_finishes_for(&self, map_id: u32) {
        loop {
            let lock = self.inner.read().await;
            if !lock.is_finish_pending(map_id) {
                return;
            }
            // Free the lock before yielding to the runtime
            drop(lock);
            tokio::task::yield_now().await;
        }
    }

    pub async fn pend(&self, map_id: u32) {
        self.inner.write().await.pend(map_id);
    }

    pub async fn release(&self, map_id: u32) {
        self.inner.write().await.release(map_id);
    }
}

impl FromRequest for FinishLocker {
    type Error = Infallible;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &actix_web::HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        ready(Ok(req
            .app_data::<Self>()
            .expect("missing finish locker")
            .clone()))
    }
}
