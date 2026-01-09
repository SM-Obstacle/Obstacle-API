#![allow(dead_code)]

use std::fmt;

use actix_http::Request;
use actix_web::{
    App, Error,
    body::MessageBody,
    dev::{Service, ServiceResponse},
    middleware, test,
};
use records_lib::Database;
use test_env::IntoResult;
use tracing_actix_web::TracingLogger;

use game_api_lib::{configure, init_env};

#[derive(Debug, serde::Deserialize)]
pub struct ErrorResponse {
    pub request_id: String,
    pub r#type: i32,
    pub message: String,
}

pub async fn with_db<F, R>(test: F) -> anyhow::Result<<R as IntoResult>::Out>
where
    F: AsyncFnOnce(Database) -> R,
    R: IntoResult,
{
    test_env::wrap(async |db| {
        init_env()?;
        test(db).await.into_result()
    })
    .await
}

pub async fn get_app(
    db: Database,
) -> impl Service<Request, Response = ServiceResponse<impl MessageBody>, Error = Error> {
    test::init_service(
        App::new()
            .wrap(middleware::from_fn(configure::fit_request_id))
            .wrap(TracingLogger::<configure::RootSpanBuilder>::new())
            .configure(|cfg| configure::configure(cfg, db.clone())),
    )
    .await
}

#[derive(Debug)]
pub enum ApiError {
    InvalidJson(Vec<u8>, serde_json::Error),
    UnexpectedJson(serde_json::Value, serde_json::Error),
    Error { r#type: i32, message: String },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::InvalidJson(raw, deser_err) => match str::from_utf8(raw) {
                Ok(s) => write!(
                    f,
                    "Invalid JSON returned by the API: {s}\nError when deserializing: {deser_err}"
                ),
                Err(_) => write!(
                    f,
                    "Invalid JSON returned by the API, with some non-UTF8 characters: {raw:?}"
                ),
            },
            ApiError::UnexpectedJson(json, deser_err) => {
                write!(
                    f,
                    "Unexpected JSON returned by the API:\n{json:#}\nError when deserializing: {deser_err}"
                )
            }
            ApiError::Error { r#type, message } => {
                f.write_str("Error returned from API: ")?;
                f.debug_map()
                    .entry(&"type", r#type)
                    .entry(&"message", message)
                    .finish()
            }
        }
    }
}

impl std::error::Error for ApiError {}

pub fn try_from_slice<'de, T>(slice: &'de [u8]) -> Result<T, ApiError>
where
    T: serde::Deserialize<'de>,
{
    match serde_json::from_slice(slice) {
        Ok(t) => Ok(t),
        Err(e) => match serde_json::from_slice::<serde_json::Value>(slice) {
            Ok(json) => match serde_json::from_value::<ErrorResponse>(json.clone()) {
                Ok(err) => Err(ApiError::Error {
                    r#type: err.r#type,
                    message: err.message.to_owned(),
                }),
                Err(_) => Err(ApiError::UnexpectedJson(json, e)),
            },
            Err(e) => Err(ApiError::InvalidJson(slice.to_vec(), e)),
        },
    }
}
