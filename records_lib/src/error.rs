use chrono::{DateTime, Utc};
use deadpool::managed::PoolError;
use thiserror::Error;
use tokio::{
    sync::{mpsc::error::SendError},
};

#[derive(Error, Debug)]
pub enum RecordsError {
    #[error(transparent)]
    MySql(#[from] sqlx::Error),
    #[error("unknown error")]
    Unknown(String),
    #[error("banned player")]
    BannedPlayer,
    #[error("unauthorized")]
    Unauthorized,
    #[error("timeout exceeded (max 5 minutes)")]
    Timeout,
    #[error("the state has already been received by the server")]
    StateAlreadyReceived(DateTime<Utc>),
    #[error("missing the /new_player_finished request")]
    MissingNewPlayerFinishReq,
    #[error("invalid ManiaPlanet access token on /gen_new_token request")]
    InvalidMPToken,
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        match self {
            Self::Unauthorized => actix_web::HttpResponse::Unauthorized().finish(),
            Self::BannedPlayer => actix_web::HttpResponse::Forbidden().body("banned player"),
            Self::Timeout => actix_web::HttpResponse::RequestTimeout().finish(),
            Self::StateAlreadyReceived(instant) => {
                actix_web::HttpResponse::BadRequest().body(format!("state already received at {:?}", instant))
            }
            Self::MissingNewPlayerFinishReq => {
                actix_web::HttpResponse::BadRequest().body("missing /new_player_finished request")
            }
            Self::InvalidMPToken => {
                actix_web::HttpResponse::BadRequest().body("invalid MP access token")
            }
            Self::MySql(err) => match err {
                sqlx::Error::Database(err) => match err.code().as_deref() {
                    Some("ER_DUP_ENTRY") => actix_web::HttpResponse::Conflict().finish(),
                    _ => actix_web::HttpResponse::InternalServerError().finish(),
                },
                _ => actix_web::HttpResponse::InternalServerError().finish(),
            },
            Self::Unknown(s) => actix_web::HttpResponse::InternalServerError().body(s.clone()),
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
