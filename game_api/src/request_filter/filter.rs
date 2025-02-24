use std::convert::Infallible;

#[cfg(feature = "request_filter")]
use actix_web::http::header;
use actix_web::{
    FromRequest,
    dev::{ConnectionInfo, RequestHead},
};
#[cfg(feature = "request_filter")]
use futures::future::Either;
use futures::future::{Ready, ready};
#[cfg(feature = "request_filter")]
use std::pin::Pin;

pub mod ingame;
pub mod website;

pub use ingame::InGameFilter;
pub use website::WebsiteFilter;

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
pub trait FromBytes: Sized {
    type Error;

    fn from_bytes(b: &[u8]) -> Result<Self, Self::Error>;
}

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
pub trait FilterAgent {
    type AgentType: FromBytes;

    fn is_valid(agent: &Self::AgentType) -> bool;
}

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
async fn flag_invalid_req<F: FilterAgent>(
    client: reqwest::Client,
    head: RequestHead,
    connection_info: ConnectionInfo,
) -> Result<CheckRequest<F>, Infallible> {
    match super::signal::send_notif(client, head, connection_info).await {
        Ok(_) => (),
        Err(e) => {
            tracing::error!("Error when trying to send invalid request notification: {e}");
        }
    }

    Ok(CheckRequest { user_agent: None })
}

pub struct CheckRequest<F: FilterAgent> {
    #[allow(dead_code)]
    pub user_agent: Option<<F as FilterAgent>::AgentType>,
}

impl<F: FilterAgent + 'static> FromRequest for CheckRequest<F> {
    type Error = Infallible;

    #[cfg(feature = "request_filter")]
    type Future = Either<
        Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>,
        Ready<Result<Self, Self::Error>>,
    >;

    #[cfg(not(feature = "request_filter"))]
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        _req: &actix_web::HttpRequest,
        _: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        #[cfg(feature = "request_filter")]
        {
            let req = _req;
            let filtered_agent = req
                .headers()
                .get(header::USER_AGENT)
                .and_then(|header| {
                    <<F as FilterAgent>::AgentType as FromBytes>::from_bytes(header.as_bytes()).ok()
                })
                .filter(<F as FilterAgent>::is_valid);

            match filtered_agent {
                user_agent @ Some(_) => Either::Right(ready(Ok(Self { user_agent }))),
                None => {
                    let client = req.app_data().cloned().unwrap();
                    Either::Left(Box::pin(flag_invalid_req::<F>(
                        client,
                        req.head().clone(),
                        req.connection_info().clone(),
                    )))
                }
            }
        }

        #[cfg(not(feature = "request_filter"))]
        {
            ready(Ok(Self { user_agent: None }))
        }
    }
}
