use std::{convert::Infallible, marker::PhantomData};

#[cfg(feature = "request_filter")]
use actix_web::http::StatusCode;
#[cfg(feature = "request_filter")]
use actix_web::http::header;
use actix_web::{
    FromRequest, HttpMessage,
    dev::{ConnectionInfo, RequestHead, Service, ServiceRequest, ServiceResponse, Transform},
};
#[cfg(feature = "request_filter")]
use futures::future::Either;
use futures::future::{Ready, ready};
#[cfg(feature = "request_filter")]
use std::pin::Pin;
#[cfg(feature = "request_filter")]
use std::task::Poll;

pub mod ingame;
pub mod website;

#[cfg_attr(not(feature = "request_filter"), allow(unused_imports))]
pub use ingame::InGameFilter;
#[cfg_attr(not(feature = "request_filter"), allow(unused_imports))]
pub use website::WebsiteFilter;

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
pub trait FromBytes: Sized {
    type Error;

    fn from_bytes(b: &[u8]) -> Result<Self, Self::Error>;
}

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
pub trait FilterAgent {
    type AgentType: FromBytes + Clone;

    fn is_valid(agent: &Self::AgentType) -> bool;
}

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
async fn flag_invalid_req(
    client: reqwest::Client,
    head: RequestHead,
    connection_info: ConnectionInfo,
) {
    match super::signal::send_notif(client, head, connection_info).await {
        Ok(_) => (),
        Err(e) => {
            tracing::error!("Error when trying to send invalid request notification: {e}");
        }
    }
}

pub struct FlagFalseRequest<F> {
    _marker: PhantomData<F>,
}

impl<F> Default for FlagFalseRequest<F> {
    fn default() -> Self {
        Self {
            _marker: Default::default(),
        }
    }
}

impl<S, B, F> Transform<S, ServiceRequest> for FlagFalseRequest<F>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>>,
    S::Future: 'static,
    F: FilterAgent + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Transform = FlagFalseRequestService<S, F>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(FlagFalseRequestService {
            service,
            _marker: PhantomData,
        }))
    }
}

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
struct RequestFlagged;

pub struct FlagFalseRequestService<S, F> {
    service: S,
    _marker: PhantomData<F>,
}

#[cfg(feature = "request_filter")]
pin_project_lite::pin_project! {
    pub struct FlagFalseRequestServiceFut<Fut, B, E> {
        #[pin]
        fut: Fut,
        invalid_req_inventory: Option<(reqwest::Client, RequestHead, ConnectionInfo)>,
        _marker: PhantomData<(B, E)>,
    }
}

#[cfg(feature = "request_filter")]
impl<Fut, B, E> Future for FlagFalseRequestServiceFut<Fut, B, E>
where
    Fut: Future<Output = Result<ServiceResponse<B>, E>>,
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = futures::ready!(this.fut.poll(cx));

        let mut res = match res {
            Ok(res) => res,
            Err(e) => return Poll::Ready(Err(e)),
        };

        match this.invalid_req_inventory.take() {
            Some((client, head, connection_info))
                if res.status() != StatusCode::NOT_FOUND
                    && !res.response().extensions().contains::<RequestFlagged>() =>
            {
                tokio::task::spawn(flag_invalid_req(client, head, connection_info));
            }
            _ => (),
        }

        res.response_mut().extensions_mut().insert(RequestFlagged);

        Poll::Ready(Ok(res))
    }
}

impl<S, B, F> Service<ServiceRequest> for FlagFalseRequestService<S, F>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>>,
    S::Future: 'static,
    F: FilterAgent + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    #[cfg(feature = "request_filter")]
    type Future = Either<FlagFalseRequestServiceFut<S::Future, B, Self::Error>, S::Future>;
    #[cfg(not(feature = "request_filter"))]
    type Future = S::Future;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        #[cfg(feature = "request_filter")]
        {
            if req.extensions().contains::<RequestFlagged>() {
                Either::Right(self.service.call(req))
            } else {
                let filtered_agent = req
                    .headers()
                    .get(header::USER_AGENT)
                    .and_then(|header| {
                        <<F as FilterAgent>::AgentType as FromBytes>::from_bytes(header.as_bytes())
                            .ok()
                    })
                    .filter(<F as FilterAgent>::is_valid);

                let invalid_req_inventory = if filtered_agent.is_some() {
                    None
                } else {
                    Some((
                        req.app_data::<reqwest::Client>().cloned().unwrap(),
                        req.head().clone(),
                        req.connection_info().clone(),
                    ))
                };

                req.extensions_mut().insert(CurrentUserAgent::<F> {
                    user_agent: filtered_agent,
                });

                let res = self.service.call(req);

                Either::Left(FlagFalseRequestServiceFut {
                    fut: res,
                    invalid_req_inventory,
                    _marker: PhantomData,
                })
            }
        }

        #[cfg(not(feature = "request_filter"))]
        {
            self.service.call(req)
        }
    }
}

pub struct CurrentUserAgent<F: FilterAgent> {
    #[allow(dead_code)]
    pub user_agent: Option<<F as FilterAgent>::AgentType>,
}

impl<F: FilterAgent> Clone for CurrentUserAgent<F> {
    fn clone(&self) -> Self {
        Self {
            user_agent: self.user_agent.clone(),
        }
    }
}

impl<F: FilterAgent + 'static> FromRequest for CurrentUserAgent<F> {
    type Error = Infallible;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &actix_web::HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        // TODO: better error system
        ready(Ok(req.extensions().get::<Self>().cloned().unwrap()))
    }
}
