use chrono::{DateTime, Utc};
use deadpool::managed::PoolError;
use deadpool_redis::redis::RedisError;
pub use deadpool_redis::Pool as RedisPool;
pub use sqlx::MySqlPool;
use std::io;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use self::models::Banishment;

mod auth;
mod graphql;
mod http;
pub mod models;
mod redis;
mod utils;

pub use auth::{AuthState, UPDATE_RATE};
pub use graphql::graphql_route;
pub use http::api_route;

#[derive(Error, Debug)]
pub enum RecordsError {
    #[error(transparent)]
    XmlError(#[from] serde_xml_rs::Error),
    #[error(transparent)]
    IOError(#[from] io::Error),
    #[error(transparent)]
    MySql(#[from] sqlx::Error),
    #[error("unknown error")]
    Unknown(String),
    #[error("banned player")]
    BannedPlayer(Banishment),
    #[error("unauthorized")]
    Unauthorized,
    #[error("timeout exceeded (max 5 minutes)")]
    Timeout,
    #[error("the state has already been received by the server")]
    StateAlreadyReceived(DateTime<Utc>),
    #[error("missing the /player/finished request with state `{0}`")]
    MissingPlayerFinishedReq(String),
    #[error("missing the /player/get_token request")]
    MissingGetTokenReq,
    #[error("invalid ManiaPlanet access token on /gen_new_token request")]
    InvalidMPToken,
    #[error("player not found in database: `{0}`")]
    PlayerNotFound(String),
    #[error("player not banned: `{0}`")]
    PlayerNotBanned(String),
    #[error("map not found in database")]
    MapNotFound(String),

    #[error("unknown role with id `{0}` and name `{1}`")]
    UnknownRole(u8, String),
    #[error("unknown medal with id `{0}` and name `{1}`")]
    UnknownMedal(u8, String),
    #[error("unknown rating kind with id `{0}` and name `{1}`")]
    UnknownRatingKind(u8, String),
    #[error("no rating found to update for player with login: `{0}` and map with uid: `{1}`")]
    NoRatingFound(String, String),
    #[error("invalid rates (too many, or repeated rate)")]
    InvalidRates,
    #[error("event `{0}` not found")]
    EventNotFound(String),
    #[error("event edition `{1}` not found for event `{0}`")]
    EventEditionNotFound(String, u32),
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        match self {
            Self::XmlError(err) => {
                actix_web::HttpResponse::InternalServerError().body(err.to_string())
            }
            Self::IOError(err) => {
                actix_web::HttpResponse::InternalServerError().body(err.to_string())
            }
            Self::Unauthorized => {
                actix_web::HttpResponse::Unauthorized().body("unauthorized action")
            }
            Self::BannedPlayer(ban) => {
                actix_web::HttpResponse::Forbidden().body(format!("banned player: {ban}"))
            }
            Self::Timeout => actix_web::HttpResponse::RequestTimeout().finish(),
            Self::StateAlreadyReceived(instant) => actix_web::HttpResponse::BadRequest()
                .body(format!("state already received at {instant:?}")),
            Self::MissingPlayerFinishedReq(s) => {
                actix_web::HttpResponse::BadRequest().body(format!("missing /player/finished request with state `{s}`"))
            }
            Self::MissingGetTokenReq => {
                actix_web::HttpResponse::BadRequest().body("missing /player/get_token request")
            }
            Self::InvalidMPToken => {
                actix_web::HttpResponse::BadRequest().body("invalid MP access token")
            }
            Self::MySql(err) => actix_web::HttpResponse::BadRequest().body(err.to_string()),
            Self::Unknown(s) => actix_web::HttpResponse::InternalServerError().body(format!(
                "unknown error: `{s}`",
            )),
            Self::PlayerNotFound(login) => actix_web::HttpResponse::BadRequest()
                .body(format!("player `{login}` not found in database")),
            Self::PlayerNotBanned(login) => actix_web::HttpResponse::BadRequest()
                .body(format!("player `{login}` is not banned")),
            Self::MapNotFound(uid) => actix_web::HttpResponse::BadRequest()
                .body(format!("map with uid `{uid}` not found in database")),
            Self::UnknownRole(id, name) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown role name `{id}`: `{name}`")),
            Self::UnknownMedal(id, name) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown medal name `{id}`: `{name}`")),
            Self::UnknownRatingKind(id, kind) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown rating kind name `{id}`: `{kind}`")),
            Self::NoRatingFound(login, map_uid) => actix_web::HttpResponse::BadRequest()
                .body(format!(
                "no rating found to update for player with login: `{login}` and map with uid: `{map_uid}`",
            )),
            Self::InvalidRates => actix_web::HttpResponse::BadRequest().body("invalid rates (too many, or repeated rate)"),
            Self::EventNotFound(handle) => actix_web::HttpResponse::BadRequest().body(format!("event `{handle}` not found")),
            Self::EventEditionNotFound(handle, edition) => actix_web::HttpResponse::BadRequest().body(format!("event edition `{edition}` not found for event `{handle}`")),
        }
    }
}

impl<E> From<PoolError<E>> for RecordsError {
    fn from(_: PoolError<E>) -> Self {
        Self::Unknown("pool error".to_owned())
    }
}

impl From<reqwest::Error> for RecordsError {
    fn from(err: reqwest::Error) -> Self {
        Self::Unknown(err.to_string())
    }
}

impl<T> From<SendError<T>> for RecordsError {
    fn from(err: SendError<T>) -> Self {
        Self::Unknown(err.to_string())
    }
}

impl From<RedisError> for RecordsError {
    fn from(err: RedisError) -> Self {
        Self::Unknown(err.to_string())
    }
}

pub type RecordsResult<T> = Result<T, RecordsError>;

#[derive(Clone)]
pub struct Database {
    pub mysql_pool: MySqlPool,
    pub redis_pool: RedisPool,
}
