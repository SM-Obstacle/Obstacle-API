use std::{
    future::{ready, Ready},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use actix_web::dev::{forward_ready, Service, ServiceRequest, Transform};
use records_lib::Database;

const REQ_CHUNK_SIZE: usize = 3;

#[derive(Default)]
pub struct ShowPoolSize {
    req_count: Arc<AtomicUsize>,
}

impl<S> Transform<S, ServiceRequest> for ShowPoolSize
where
    S: Service<ServiceRequest>,
{
    type Response = <S as Service<ServiceRequest>>::Response;

    type Error = <S as Service<ServiceRequest>>::Error;

    type Transform = ShowPoolSizeMw<S>;

    type InitError = ();

    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ShowPoolSizeMw {
            next: service,
            count: self.req_count.clone(),
        }))
    }
}

pub struct ShowPoolSizeMw<S> {
    next: S,
    count: Arc<AtomicUsize>,
}

impl<S> Service<ServiceRequest> for ShowPoolSizeMw<S>
where
    S: Service<ServiceRequest>,
{
    type Response = <S as Service<ServiceRequest>>::Response;

    type Error = <S as Service<ServiceRequest>>::Error;

    type Future = <S as Service<ServiceRequest>>::Future;

    forward_ready!(next);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let chunk_idx = self
            .count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
                Some((x + 1) % REQ_CHUNK_SIZE)
            })
            .unwrap();

        if chunk_idx == 0 {
            let db = req.app_data::<Database>().unwrap();
            tracing::info!(
                "Pool size: {}; Pool idle amount: {}",
                db.mysql_pool.size(),
                db.mysql_pool.num_idle()
            );
        }

        <S as Service<ServiceRequest>>::call(&self.next, req)
    }
}
