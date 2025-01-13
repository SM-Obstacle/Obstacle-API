use std::fmt;

use actix_web::HttpResponse;
use records_lib::{models, NullableInteger};
use tokio::sync::mpsc::error::SendError;
use tracing_actix_web::RequestId;

#[derive(serde::Deserialize, Debug, Clone)]
#[allow(unused)] // not used in code, but displayed as debug
pub struct AccessTokenErr {
    error: String,
    message: String,
    hint: String,
}

#[derive(thiserror::Error, Debug)]
#[repr(i32)] // i32 to be used with clients that don't support unsigned integers
#[rustfmt::skip]
pub enum RecordsErrorKind {
    // Caution: when creating a new error, you must ensure its code isn't
    // in conflict with another one in `records_lib::RecordsError`.

    // --------
    // --- Internal server errors
    // --------

    #[error(transparent)]
    IOError(#[from] std::io::Error) = 101,

    // ...Errors from records_lib

    #[error("unknown error: {0}")]
    Unknown(String) = 105,
    #[error("server is in maintenance since {0}")]
    Maintenance(chrono::NaiveDateTime) = 106,
    #[error("unknown api status: `{0}` named `{1}`")]
    UnknownStatus(u8, String) = 107,

    // ...Errors from records_lib

    // --------
    // --- Authentication errors
    // --------

    #[error("unauthorized")]
    Unauthorized = 201,
    #[error("forbidden")]
    Forbidden = 202,
    #[error("missing the /player/get_token request")]
    MissingGetTokenReq = 203,
    #[error("the state has already been received by the server")]
    StateAlreadyReceived(chrono::DateTime<chrono::Utc>) = 204,
    #[error("banned player {0}")]
    BannedPlayer(models::Banishment) = 205,
    #[error("error on sending request to MP services: {0:?}")]
    AccessTokenErr(AccessTokenErr) = 206,
    #[error("invalid ManiaPlanet code on /player/give_token request")]
    InvalidMPCode = 207,
    #[error("timeout exceeded")]
    Timeout(std::time::Duration) = 208,

    // --------
    // --- Logical errors
    // --------

    #[error("not found")]
    EndpointNotFound = 301,

    // ...Error from records_lib

    #[error("player not banned: `{0}`")]
    PlayerNotBanned(String) = 303,

    // ...Error from records_lib

    #[error("unknown role with id `{0}` and name `{1}`")]
    UnknownRole(u8, String) = 305,
    #[error("unknown rating kind with id `{0}` and name `{1}`")]
    UnknownRatingKind(u8, String) = 307,
    #[error("no rating found to update for player with login: `{0}` and map with uid: `{1}`")]
    NoRatingFound(String, String) = 308,
    #[error("invalid rates (too many, or repeated rate)")]
    InvalidRates = 309,

    // ...Errors from records_lib

    #[error("invalid times")]
    InvalidTimes = 313,
    #[error("map pack id should be an integer, got `{0}`")]
    InvalidMappackId(String),
    #[error("event `{0}` {1} has expired")]
    EventHasExpired(String, u32),

    #[error(transparent)]
    Lib(#[from] records_lib::error::RecordsError),
}

impl RecordsErrorKind {
    pub fn get_type(&self) -> i32 {
        match self {
            Self::Lib(err) => err.get_code(),
            // SAFETY: Self is repr(i32).
            other => unsafe { *(other as *const Self as *const _) },
        }
    }
}

impl From<sqlx::Error> for RecordsErrorKind {
    fn from(value: sqlx::Error) -> Self {
        Self::Lib(value.into())
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
    #[inline(always)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.kind, f)
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
            r#type: self.kind.get_type(),
            message,
        }
    }
}

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    pub request_id: String,
    pub r#type: i32,
    pub message: String,
}

impl actix_web::ResponseError for RecordsError {
    #[rustfmt::skip]
    fn error_response(&self) -> HttpResponse {
        use records_lib::error::RecordsError as LR;
        use RecordsErrorKind as R;

        match &self.kind {
            // --- Internal server errors

            R::IOError(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Unknown(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Maintenance(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::UnknownStatus(..) => HttpResponse::InternalServerError().json(self.to_err_res()),

            // --- Authentication errors

            R::Unauthorized => HttpResponse::Unauthorized().json(self.to_err_res()),
            R::Forbidden => HttpResponse::Forbidden().json(self.to_err_res()),
            R::MissingGetTokenReq => HttpResponse::BadRequest().json(self.to_err_res()),
            R::StateAlreadyReceived(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::BannedPlayer(ban) => {
                #[derive(serde::Serialize)]
                struct Ban<'a> {
                    #[serde(flatten)]
                    ban: &'a models::Banishment,
                    duration: NullableInteger,
                }

                #[derive(serde::Serialize)]
                struct BannedPlayerResponse<'a> {
                    message: String,
                    ban: Ban<'a>,
                }
                HttpResponse::Forbidden().json(BannedPlayerResponse {
                    message: ban.to_string(),
                    ban: Ban {
                        ban,
                        duration: NullableInteger(ban.duration.map(|x| x as _)),
                    },
                })
            }
            R::AccessTokenErr(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidMPCode => HttpResponse::BadRequest().json(self.to_err_res()),
            R::Timeout(_) => HttpResponse::RequestTimeout().json(self.to_err_res()),

            // --- Logical errors

            R::EndpointNotFound => HttpResponse::NotFound().json(self.to_err_res()),
            R::PlayerNotBanned(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::UnknownRole(..) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::UnknownRatingKind(..) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::NoRatingFound(..) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidRates => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidTimes => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidMappackId(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::EventHasExpired(..) => HttpResponse::BadRequest().json(self.to_err_res()),

            // --- From the library

            R::Lib(e) => match e {
                // --- Internal server errors

                LR::MySql(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
                LR::Redis(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
                LR::ExternalRequest(_) => {
                    HttpResponse::InternalServerError().json(self.to_err_res())
                }
                LR::PoolError(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
                LR::Internal => HttpResponse::InternalServerError().json(self.to_err_res()),

                // --- Logical errors

                LR::PlayerNotFound(_) => HttpResponse::BadRequest().json(self.to_err_res()),
                LR::MapNotFound(_) => HttpResponse::BadRequest().json(self.to_err_res()),
                LR::EventNotFound(_) => HttpResponse::BadRequest().json(self.to_err_res()),
                LR::EventEditionNotFound(..) => HttpResponse::BadRequest().json(self.to_err_res()),
                LR::MapNotInEventEdition(..) => HttpResponse::BadRequest().json(self.to_err_res()),
            },
        }
    }
}

pub type RecordsResult<T> = Result<T, RecordsErrorKind>;

pub type RecordsResponse<T> = Result<T, RecordsError>;

pub trait FitRequestId<T, E> {
    fn fit(self, request_id: RequestId) -> RecordsResponse<T>;
}

impl<T, E> FitRequestId<T, E> for Result<T, E>
where
    RecordsErrorKind: From<E>,
{
    fn fit(self, request_id: RequestId) -> RecordsResponse<T> {
        self.map_err(|e| RecordsError {
            request_id,
            kind: e.into(),
        })
    }
}

/// Converts a `Result<T, E>` in which `E` is convertible to [`records_lib::error::RecordsError`]
/// into a [`RecordsResult<T>`].
pub trait RecordsResultExt<T> {
    fn with_api_err(self) -> RecordsResult<T>;
}

impl<T, E> RecordsResultExt<T> for Result<T, E>
where
    records_lib::error::RecordsError: From<E>,
{
    fn with_api_err(self) -> RecordsResult<T> {
        self.map_err(records_lib::error::RecordsError::from)
            .map_err(Into::into)
    }
}
