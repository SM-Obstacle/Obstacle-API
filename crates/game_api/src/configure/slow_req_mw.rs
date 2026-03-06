use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
};
use futures::future::LocalBoxFuture;
use std::future::{Ready, ready};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
pub struct WebhookTimeoutHandler;

impl TimeoutHandler for WebhookTimeoutHandler {
    fn on_timeout(&self, _: &TimeoutInfo) {
        // TODO: webhook handler
    }
}

#[derive(Clone)]
pub struct TracingTimeoutHandler;

impl TimeoutHandler for TracingTimeoutHandler {
    fn on_timeout(&self, info: &TimeoutInfo) {
        tracing::warn!(
            "Request to {} took more than {}ms",
            info.endpoint_path,
            info.timeout.as_micros()
        );
    }
}

#[derive(Clone)]
pub struct SequentialTimeoutHandler<H1, H2> {
    handler1: H1,
    handler2: H2,
}

impl<H1, H2> TimeoutHandler for SequentialTimeoutHandler<H1, H2>
where
    H1: TimeoutHandler,
    H2: TimeoutHandler,
{
    fn on_timeout(&self, info: &TimeoutInfo) {
        self.handler1.on_timeout(info);
        self.handler2.on_timeout(info);
    }
}

pub struct TimeoutInfo {
    pub endpoint_path: String,
    pub timeout: Duration,
}

pub trait TimeoutHandler {
    fn on_timeout(&self, info: &TimeoutInfo);

    fn chain_with<H2>(self, second_handler: H2) -> SequentialTimeoutHandler<Self, H2>
    where
        H2: TimeoutHandler,
        Self: Sized,
    {
        SequentialTimeoutHandler {
            handler1: self,
            handler2: second_handler,
        }
    }
}

pub struct RequestTimeoutNotifier<H> {
    timeout: Duration,
    handler: H,
}

impl<H> RequestTimeoutNotifier<H> {
    pub fn new(timeout: Duration, handler: H) -> Self {
        Self { timeout, handler }
    }
}

impl<S, B, H> Transform<S, ServiceRequest> for RequestTimeoutNotifier<H>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    H: TimeoutHandler + Clone + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestTimeoutNotifierMiddleware<S, H>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestTimeoutNotifierMiddleware {
            service,
            timeout: self.timeout,
            handler: self.handler.clone(),
        }))
    }
}

#[doc(hidden)]
pub struct RequestTimeoutNotifierMiddleware<S, H> {
    service: S,
    timeout: Duration,
    handler: H,
}

impl<S, B, H> Service<ServiceRequest> for RequestTimeoutNotifierMiddleware<S, H>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    H: TimeoutHandler + Clone + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path().to_owned();
        let timeout_info = TimeoutInfo {
            endpoint_path: path,
            timeout: self.timeout,
        };

        let timeout = self.timeout;
        let handler = self.handler.clone();

        let fut = self.service.call(req);

        Box::pin(async move {
            tokio::pin!(fut);

            tokio::select! {
                _ = sleep(timeout) => {
                    handler.on_timeout(&timeout_info);
                    fut.await
                }
                res = &mut fut => {
                    res
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::OnceLock, time::Duration};

    use actix_web::{App, HttpResponse, test, web};

    use crate::configure::slow_req_mw::TimeoutHandler;

    #[tokio::test]
    async fn slow_req_mw_catch() {
        static TIMEOUT_CATCHED: OnceLock<()> = OnceLock::new();

        #[derive(Clone)]
        struct OnTimeout;
        impl TimeoutHandler for OnTimeout {
            fn on_timeout(&self, _: &super::TimeoutInfo) {
                let _ = TIMEOUT_CATCHED.set(());
            }
        }

        let app = test::init_service(
            App::new()
                .wrap(super::RequestTimeoutNotifier::new(
                    Duration::from_secs(1),
                    OnTimeout,
                ))
                .default_service(web::to(|| async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    HttpResponse::Ok().finish()
                })),
        )
        .await;
        let req = test::TestRequest::default().to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        assert!(TIMEOUT_CATCHED.get().is_some());
    }

    #[tokio::test]
    async fn slow_req_mw_pass() {
        static TIMEOUT_CATCHED: OnceLock<()> = OnceLock::new();

        #[derive(Clone)]
        struct OnTimeout;
        impl TimeoutHandler for OnTimeout {
            fn on_timeout(&self, _: &super::TimeoutInfo) {
                let _ = TIMEOUT_CATCHED.set(());
            }
        }

        let app = test::init_service(
            App::new()
                .wrap(super::RequestTimeoutNotifier::new(
                    Duration::from_secs(2),
                    OnTimeout,
                ))
                .default_service(web::to(|| async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    HttpResponse::Ok().finish()
                })),
        )
        .await;
        let req = test::TestRequest::default().to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        assert!(TIMEOUT_CATCHED.get().is_none());
    }
}
