use chrono::{DateTime, Utc};
use deadpool::managed::PoolError;
use std::io;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::models::Banishment;

#[derive(Error, Debug)]
pub enum RecordsError {
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
    #[error("missing the /new_player_finished request")]
    MissingPlayerFinishedReq,
    #[error("invalid ManiaPlanet access token on /gen_new_token request")]
    InvalidMPToken,
    #[error("player not found in database")]
    PlayerNotFound(String),
    #[error("map not found in database")]
    MapNotFound(String),

    #[error("unknown role with id `{0}` and name `{1}`")]
    UnknownRole(u8, String),
    #[error("unknown medal with id `{0}` and name `{1}`")]
    UnknownMedal(u8, String),
    #[error("unknown rating kind with id `{0}` and name `{1}`")]
    UnknownRatingKind(u8, String),
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        match self {
            Self::IOError(err) => {
                actix_web::HttpResponse::InternalServerError().body(err.to_string())
            }
            Self::Unauthorized => actix_web::HttpResponse::Unauthorized().finish(),
            Self::BannedPlayer(ban) => {
                actix_web::HttpResponse::Forbidden().body(format!("banned player: {ban}"))
            }
            Self::Timeout => actix_web::HttpResponse::RequestTimeout().finish(),
            Self::StateAlreadyReceived(instant) => actix_web::HttpResponse::BadRequest()
                .body(format!("state already received at {instant:?}")),
            Self::MissingPlayerFinishedReq => {
                actix_web::HttpResponse::BadRequest().body("missing /player_finished request")
            }
            Self::InvalidMPToken => {
                actix_web::HttpResponse::BadRequest().body("invalid MP access token")
            }
            Self::MySql(err) => actix_web::HttpResponse::BadRequest().body(err.to_string()),
            Self::Unknown(s) => actix_web::HttpResponse::InternalServerError().body(s.clone()),
            Self::PlayerNotFound(login) => actix_web::HttpResponse::BadRequest()
                .body(format!("player `{login}` not found in database")),
            Self::MapNotFound(uid) => actix_web::HttpResponse::BadRequest()
                .body(format!("map with uid `{uid}` not found in database")),
            Self::UnknownRole(id, name) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown role name `{id}`: `{name}`")),
            Self::UnknownMedal(id, name) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown medal name `{id}`: `{name}`")),
            Self::UnknownRatingKind(id, kind) => actix_web::HttpResponse::InternalServerError()
                .body(format!("unknown rating kind name `{id}`: `{kind}`")),
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
