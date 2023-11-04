use chrono::{DateTime, Utc};
use core::fmt;
use deadpool::managed::PoolError;
use deadpool_redis::redis::RedisError;
pub use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::mysql;
pub use sqlx::MySqlPool;
use std::fmt::Debug;
use std::{io, time::Duration};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tracing_actix_web::RequestId;

use self::models::Banishment;

mod auth;
mod graphql;
mod http;
pub mod models;
pub(crate) mod must;
mod redis;
mod utils;

pub use auth::{get_tokens_ttl, AuthState};
pub use graphql::graphql_route;
pub use http::api_route;
pub use utils::{get_env_var, get_env_var_as, read_env_var_file};

#[derive(Deserialize, Debug, Clone)]
#[allow(unused)] // not used in code, but displayed as debug
pub struct AccessTokenErr {
    error: String,
    message: String,
    hint: String,
}

#[derive(Error, Debug)]
#[repr(i32)]
pub enum RecordsErrorKind {
    // Internal server errors
    #[error(transparent)]
    IOError(#[from] io::Error) = 101,
    #[error(transparent)]
    MySql(#[from] sqlx::Error) = 102,
    #[error(transparent)]
    Redis(#[from] RedisError) = 103,
    #[error(transparent)]
    ExternalRequest(#[from] reqwest::Error) = 104,
    #[error("unknown error: {0}")]
    Unknown(String) = 105,
    #[error("server is in maintenance since {0}")]
    Maintenance(chrono::NaiveDateTime) = 106,
    #[error("unknown api status: `{0}` named `{1}`")]
    UnknownStatus(u8, String) = 107,

    // Authentication errors
    #[error("unauthorized")]
    Unauthorized = 201,
    #[error("forbidden")]
    Forbidden = 202,
    #[error("missing the /player/get_token request")]
    MissingGetTokenReq = 203,
    #[error("the state has already been received by the server")]
    StateAlreadyReceived(DateTime<Utc>) = 204,
    #[error("banned player {0}")]
    BannedPlayer(Banishment) = 205,
    #[error("error on sending request to MP services: {0:?}")]
    AccessTokenErr(AccessTokenErr) = 206,
    #[error("invalid ManiaPlanet code on /player/give_token request")]
    InvalidMPCode = 207,
    #[error("timeout exceeded (max 5 minutes)")]
    Timeout = 208,

    // Logical errors
    #[error("not found")]
    EndpointNotFound = 301,
    #[error("player not found in database: `{0}`")]
    PlayerNotFound(String) = 302,
    #[error("player not banned: `{0}`")]
    PlayerNotBanned(String) = 303,
    #[error("map not found in database")]
    MapNotFound(String) = 304,
    #[error("unknown role with id `{0}` and name `{1}`")]
    UnknownRole(u8, String) = 305,
    #[error("unknown medal with id `{0}` and name `{1}`")]
    UnknownMedal(u8, String) = 306,
    #[error("unknown rating kind with id `{0}` and name `{1}`")]
    UnknownRatingKind(u8, String) = 307,
    #[error("no rating found to update for player with login: `{0}` and map with uid: `{1}`")]
    NoRatingFound(String, String) = 308,
    #[error("invalid rates (too many, or repeated rate)")]
    InvalidRates = 309,
    #[error("event `{0}` not found")]
    EventNotFound(String) = 310,
    #[error("event edition `{1}` not found for event `{0}`")]
    EventEditionNotFound(String, u32) = 311,
    #[error("map with uid `{0}` is not registered for event `{1}` edition {2}")]
    MapNotInEventEdition(String, String, u32) = 312,
    #[error("invalid times")]
    InvalidTimes = 313,
}

impl<E: Debug> From<PoolError<E>> for RecordsErrorKind {
    fn from(value: PoolError<E>) -> Self {
        Self::Unknown(format!("pool error: {value:?}"))
    }
}

impl<T> From<SendError<T>> for RecordsErrorKind {
    fn from(value: SendError<T>) -> Self {
        Self::Unknown(format!("send error: {value:?}"))
    }
}

#[derive(Debug)]
pub struct RecordsError {
    pub request_id: RequestId,
    pub kind: RecordsErrorKind,
}

impl fmt::Display for RecordsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} --- request id: {}", self.kind, self.request_id)
    }
}

impl std::error::Error for RecordsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

impl RecordsError {
    fn to_err_res(&self) -> ErrorResponse {
        let message = self.to_string();
        ErrorResponse {
            request_id: self.request_id.to_string(),
            r#type: unsafe { *(&self.kind as *const RecordsErrorKind as *const _) },
            message,
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    request_id: String,
    r#type: i32,
    message: String,
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use RecordsErrorKind as R;

        match &self.kind {
            // Internal server errors
            R::IOError(_) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res()),
            R::MySql(_) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Redis(_) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res()),
            R::ExternalRequest(_) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }
            R::Unknown(_) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Maintenance(_) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }
            R::UnknownStatus(..) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }

            // Authentication errors
            R::Unauthorized => actix_web::HttpResponse::Unauthorized().json(self.to_err_res()),
            R::Forbidden => actix_web::HttpResponse::Forbidden().json(self.to_err_res()),
            R::MissingGetTokenReq => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::StateAlreadyReceived(_) => {
                actix_web::HttpResponse::BadRequest().json(self.to_err_res())
            }
            R::BannedPlayer(ban) => {
                #[derive(Serialize)]
                struct BannedPlayerResponse<'a> {
                    message: String,
                    ban: &'a Banishment,
                }
                actix_web::HttpResponse::Forbidden().json(BannedPlayerResponse {
                    message: ban.to_string(),
                    ban,
                })
            }
            R::AccessTokenErr(_) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidMPCode => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::Timeout => actix_web::HttpResponse::RequestTimeout().json(self.to_err_res()),

            // Logical errors
            R::EndpointNotFound => actix_web::HttpResponse::NotFound().json(self.to_err_res()),
            R::PlayerNotFound(_) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::PlayerNotBanned(_) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::MapNotFound(_) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::UnknownRole(..) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }
            R::UnknownMedal(..) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }
            R::UnknownRatingKind(..) => {
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res())
            }
            R::NoRatingFound(..) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidRates => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::EventNotFound(_) => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
            R::EventEditionNotFound(..) => {
                actix_web::HttpResponse::BadRequest().json(self.to_err_res())
            }
            R::MapNotInEventEdition(..) => {
                actix_web::HttpResponse::BadRequest().json(self.to_err_res())
            }
            R::InvalidTimes => actix_web::HttpResponse::BadRequest().json(self.to_err_res()),
        }
    }
}

pub type RecordsResult<T> = Result<T, RecordsErrorKind>;

pub type RecordsResponse<T> = Result<T, RecordsError>;

#[derive(Clone)]
pub struct Database {
    pub mysql_pool: MySqlPool,
    pub redis_pool: RedisPool,
}

pub async fn get_mysql_pool() -> anyhow::Result<MySqlPool> {
    let mysql_pool = mysql::MySqlPoolOptions::new().acquire_timeout(Duration::from_secs(10));

    #[cfg(feature = "localhost_test")]
    let url = &get_env_var("DATABASE_URL");
    #[cfg(not(feature = "localhost_test"))]
    let url = &read_env_var_file("DATABASE_URL");

    let mysql_pool = mysql_pool.connect(url).await?;
    Ok(mysql_pool)
}

pub trait FitRequestId<T, E> {
    fn fit(self, request_id: &RequestId) -> RecordsResponse<T>;
}

impl<T, E> FitRequestId<T, E> for Result<T, E>
where
    RecordsErrorKind: From<E>,
{
    fn fit(self, request_id: &RequestId) -> RecordsResponse<T> {
        self.map_err(|e| RecordsError {
            request_id: *request_id,
            kind: e.into(),
        })
    }
}
