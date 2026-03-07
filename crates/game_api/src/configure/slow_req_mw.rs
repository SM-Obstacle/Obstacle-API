use actix_http::RequestHead;
use actix_web::{
    Error,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
};
use dsc_webhook::{FormattedRequestHead, WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};
use mkenv::Layer as _;
use std::{
    fmt,
    future::{Ready, ready},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time;
use tracing_actix_web::RequestId;

#[derive(Clone)]
pub struct WebhookTimeoutHandler(pub reqwest::Client);

impl TimeoutHandler for WebhookTimeoutHandler {
    fn on_timeout(&self, info: &TimeoutInfo) {
        let wh_msg = WebhookBody {
            content: "Request timed out".to_owned(),
            embeds: vec![WebhookBodyEmbed {
                title: "Request".to_owned(),
                description: None,
                color: 5814783,
                fields: Some(vec![
                    WebhookBodyEmbedField {
                        name: "Head".to_owned(),
                        value: format!(
                            "```\n{}\n```",
                            FormattedRequestHead::new(&info.request_head)
                        ),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Request ID".to_owned(),
                        value: format!("```{}```", OptReqIdFmt(info.request_id.as_ref())),
                        inline: None,
                    },
                ]),
                url: None,
            }],
        };

        let client = self.0.clone();

        tokio::task::spawn(async move {
            if let Err(e) = client
                .post(crate::env().wh_request_timeout.get())
                .json(&wh_msg)
                .send()
                .await
            {
                tracing::error!("couldn't send timeout error to webhook: {e}. body:\n{wh_msg:#?}");
            }
        });
    }
}

struct OptReqIdFmt<'a>(Option<&'a RequestId>);

impl fmt::Display for OptReqIdFmt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(req_id) => fmt::Display::fmt(req_id, f),
            None => f.write_str("none"),
        }
    }
}

#[derive(Clone)]
pub struct TracingTimeoutHandler;

impl TimeoutHandler for TracingTimeoutHandler {
    fn on_timeout(&self, info: &TimeoutInfo) {
        tracing::warn!(
            "Request [{req_id}] {method} {uri} took more than {timeout}ms",
            req_id = OptReqIdFmt(info.request_id.as_ref()),
            method = info.request_head.method,
            uri = info.request_head.uri,
            timeout = info.timeout.as_millis(),
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
    pub request_head: RequestHead,
    pub timeout: Duration,
    pub request_id: Option<RequestId>,
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
    type Future = TimeoutNotifierFuture<S::Future, H>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        TimeoutNotifierFuture {
            handler: self.handler.clone(),
            sleep_fut: time::sleep(self.timeout),
            info: TimeoutInfo {
                request_head: req.head().clone(),
                timeout: self.timeout,
                request_id: req.app_data().copied(),
            },
            is_handled: false,
            inner_fut: self.service.call(req),
        }
    }
}

pin_project_lite::pin_project! {
    pub struct TimeoutNotifierFuture<F, H> {
        #[pin]
        inner_fut: F,
        #[pin]
        sleep_fut: time::Sleep,
        info: TimeoutInfo,
        handler: H,
        is_handled: bool,
    }
}

impl<F, H, T> Future for TimeoutNotifierFuture<F, H>
where
    F: Future<Output = T>,
    H: TimeoutHandler,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if let Poll::Ready(res) = this.inner_fut.poll(cx) {
            return Poll::Ready(res);
        }

        if !*this.is_handled
            && let Poll::Ready(_) = this.sleep_fut.poll(cx)
        {
            this.handler.on_timeout(this.info);
            *this.is_handled = true;
        }

        Poll::Pending
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
                TIMEOUT_CATCHED
                    .set(())
                    .unwrap_or_else(|_| panic!("on_timeout should be called only once"));
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
                TIMEOUT_CATCHED
                    .set(())
                    .unwrap_or_else(|_| panic!("on_timeout should be called only once"));
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
