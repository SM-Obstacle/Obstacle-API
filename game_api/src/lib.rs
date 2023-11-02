use chrono::{DateTime, Utc};
use deadpool::managed::PoolError;
use deadpool_redis::redis::RedisError;
pub use deadpool_redis::Pool as RedisPool;
use serde::{Serialize, Deserialize};
pub use sqlx::MySqlPool;
use sqlx::mysql;
use std::{io, time::Duration};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use std::fmt::Debug;

use self::models::Banishment;

mod auth;
mod graphql;
mod http;
pub mod models;
mod redis;
mod utils;
pub(crate) mod must;

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
pub enum RecordsError {
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

impl RecordsError {
    fn to_err_res(&self, message: String) -> ErrorResponse {
        ErrorResponse {
            r#type: unsafe { *(self as *const Self as *const i32) },
            message
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    r#type: i32,
    message: String,
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        match self {
            // Internal server errors
            Self::IOError(err) => 
                actix_web::HttpResponse::InternalServerError().json(self.to_err_res(err.to_string())),
            Self::MySql(err) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res(err.to_string())),
            Self::Redis(err) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res(err.to_string())),
            Self::ExternalRequest(err) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res(err.to_string())),
            Self::Unknown(s) => actix_web::HttpResponse::InternalServerError().json(self.to_err_res(format!(
                "unknown error: `{s}`",
            ))),

            // Authentication errors
            Self::Unauthorized => {
                actix_web::HttpResponse::Unauthorized().json(self.to_err_res("unauthorized action".to_owned()))
            }
            Self::Forbidden => actix_web::HttpResponse::Forbidden().json(self.to_err_res("forbidden action".to_owned())),
            Self::MissingGetTokenReq => {
                actix_web::HttpResponse::BadRequest().json(self.to_err_res( "missing /player/get_token request".to_owned()))
            }
            Self::StateAlreadyReceived(instant) => actix_web::HttpResponse::BadRequest()
                .json(self.to_err_res(format!("state already received at {instant:?}"))),
            Self::BannedPlayer(ban) => {
                #[derive(Serialize)]
                struct BannedPlayerResponse<'a> {
                    message: String,
                    ban: &'a Banishment,
                }
                actix_web::HttpResponse::Forbidden().json(BannedPlayerResponse { message: ban.to_string(), ban })
            }
            Self::AccessTokenErr(err) =>
                actix_web::HttpResponse::BadRequest().json(self.to_err_res(format!("{err:?}"))),
            Self::InvalidMPCode => {
                actix_web::HttpResponse::BadRequest().json(self.to_err_res("invalid MP code".to_owned()))
            }
            Self::Timeout => actix_web::HttpResponse::RequestTimeout().json(self.to_err_res("timeout exceeded (max 5 min)".to_owned())),

            // Logical errors
            Self::EndpointNotFound => actix_web::HttpResponse::NotFound().json(self.to_err_res("not found".to_owned())),
            Self::PlayerNotFound(login) => actix_web::HttpResponse::BadRequest()
                .json(self.to_err_res(format!("player `{login}` not found in database"))),
            Self::PlayerNotBanned(login) => actix_web::HttpResponse::BadRequest()
                .json(self.to_err_res(format!("player `{login}` is not banned"))),
            Self::MapNotFound(uid) => actix_web::HttpResponse::BadRequest()
                .json(self.to_err_res(format!("map with uid `{uid}` not found in database"))),
            Self::UnknownRole(id, name) => actix_web::HttpResponse::InternalServerError()
                .json(self.to_err_res(format!("unknown role name `{id}`: `{name}`"))),
            Self::UnknownMedal(id, name) => actix_web::HttpResponse::InternalServerError()
                .json(self.to_err_res(format!("unknown medal name `{id}`: `{name}`"))),
            Self::UnknownRatingKind(id, kind) => actix_web::HttpResponse::InternalServerError()
                .json(self.to_err_res(format!("unknown rating kind name `{id}`: `{kind}`"))),
            Self::NoRatingFound(login, map_uid) => actix_web::HttpResponse::BadRequest()
                .json(self.to_err_res(format!(
                "no rating found to update for player with login: `{login}` and map with uid: `{map_uid}`",
            ))),
            Self::InvalidRates => actix_web::HttpResponse::BadRequest().json(self.to_err_res("invalid rates (too many, or repeated rate)".to_owned())),
            Self::EventNotFound(handle) => actix_web::HttpResponse::BadRequest().json(self.to_err_res(format!("event `{handle}` not found"))),
            Self::EventEditionNotFound(handle, edition) => actix_web::HttpResponse::BadRequest().json(self.to_err_res(format!("event edition `{edition}` not found for event `{handle}`"))),
            Self::MapNotInEventEdition(uid, handle, edition) => actix_web::HttpResponse::BadRequest().json(self.to_err_res(format!("map with uid `{uid}` is not registered for event `{handle}` edition {edition}"))),
            Self::InvalidTimes => actix_web::HttpResponse::BadRequest().json(self.to_err_res("invalid times (total run time may not be equal to the sum of all checkpoint times)".to_owned())),
        }
    }
}

impl<E: Debug> From<PoolError<E>> for RecordsError {
    fn from(err: PoolError<E>) -> Self {
        Self::Unknown(format!("pool error: `{err:?}`"))
    }
}

impl<T> From<SendError<T>> for RecordsError {
    fn from(err: SendError<T>) -> Self {
        Self::Unknown(format!("send error: `{:?}`", err.to_string()))
    }
}

pub type RecordsResult<T> = Result<T, RecordsError>;

#[derive(Clone)]
pub struct Database {
    pub mysql_pool: MySqlPool,
    pub redis_pool: RedisPool,
}

pub async fn get_mysql_pool() -> RecordsResult<MySqlPool> {
    let mysql_pool = mysql::MySqlPoolOptions::new().acquire_timeout(Duration::from_secs(10));

    #[cfg(feature = "localhost_test")]
    let url = &get_env_var("DATABASE_URL");
    #[cfg(not(feature = "localhost_test"))]
    let url = &read_env_var_file("DATABASE_URL");

    let mysql_pool = mysql_pool
        .connect(url)
        .await?;
    Ok(mysql_pool)
}