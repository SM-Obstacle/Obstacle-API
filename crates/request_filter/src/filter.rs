use std::{convert::Infallible, marker::PhantomData};

use actix_web::http::StatusCode;
use actix_web::http::header;
use actix_web::{
    FromRequest, HttpMessage,
    dev::{ConnectionInfo, RequestHead, Service, ServiceRequest, ServiceResponse, Transform},
};
use futures::future::{Ready, ready};
use std::pin::Pin;
use std::task::Poll;

pub mod ingame;
pub mod website;

pub use ingame::InGameFilter;
pub use website::WebsiteFilter;

pub trait FilterAgent {
    type AgentType: for<'a> TryFrom<&'a [u8]> + Clone;

    fn is_valid(agent: &Self::AgentType) -> bool;
}

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

struct RequestFlagged;

pub struct FlagFalseRequestService<S, F> {
    service: S,
    _marker: PhantomData<F>,
}

pin_project_lite::pin_project! {
    pub struct FlagFalseRequestServiceFut<Fut, B, E> {
        #[pin]
        fut: Fut,
        invalid_req_inventory: Option<(reqwest::Client, RequestHead, ConnectionInfo)>,
        _marker: PhantomData<(B, E)>,
    }
}

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
    type Future = FlagFalseRequestServiceFut<S::Future, B, Self::Error>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let filtered_agent = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|header| {
                <<F as FilterAgent>::AgentType as TryFrom<&[u8]>>::try_from(header.as_bytes()).ok()
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

        FlagFalseRequestServiceFut {
            fut: res,
            invalid_req_inventory,
            _marker: PhantomData,
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
