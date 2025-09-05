use std::fmt;

use actix_web::{HttpResponse, http::StatusCode};
use entity::banishments;
use sea_orm::DbErr;
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
    #[error("banned player")]
    BannedPlayer(banishments::Model) = 205,
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

#[derive(serde::Serialize)]
pub struct RecordsErrorKindResponse {
    pub r#type: i32,
    pub message: String,
}

impl actix_web::ResponseError for RecordsErrorKind {
    fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
        let (r#type, status_code) = self.get_err_type_and_status_code();
        let mut res = HttpResponse::build(status_code);

        let message = self.to_string();
        res.extensions_mut().insert(Some(RecordsErrorKindResponse {
            r#type,
            message: message.clone(),
        }));

        res.json(RecordsErrorKindResponse { r#type, message })
    }
}

impl RecordsErrorKind {
    pub fn get_err_type_and_status_code(&self) -> (i32, StatusCode) {
        use RecordsErrorKind as E;
        use StatusCode as S;
        use records_lib::error::RecordsError as LE;

        match self {
            E::IOError(_) => (101, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::MySql(_)) => (102, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::Redis(_)) => (103, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::ExternalRequest(_)) => (104, S::INTERNAL_SERVER_ERROR),
            E::Unknown(_) => (105, S::INTERNAL_SERVER_ERROR),
            E::Maintenance(_) => (106, S::INTERNAL_SERVER_ERROR),
            E::UnknownStatus(_, _) => (107, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::PoolError(_)) => (108, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::Internal) => (109, S::INTERNAL_SERVER_ERROR),
            E::Lib(LE::DbError(_)) => (110, S::INTERNAL_SERVER_ERROR),

            E::Unauthorized => (201, S::UNAUTHORIZED),
            E::Forbidden => (202, S::FORBIDDEN),
            E::MissingGetTokenReq => (203, S::BAD_REQUEST),
            E::StateAlreadyReceived(_) => (204, S::BAD_REQUEST),
            E::BannedPlayer(_) => (205, S::FORBIDDEN),
            E::AccessTokenErr(_) => (206, S::BAD_REQUEST),
            E::InvalidMPCode => (207, S::BAD_REQUEST),
            E::Timeout(_) => (208, S::REQUEST_TIMEOUT),

            E::EndpointNotFound => (301, S::NOT_FOUND),
            E::Lib(LE::PlayerNotFound(_)) => (302, S::BAD_REQUEST),
            E::PlayerNotBanned(_) => (303, S::BAD_REQUEST),
            E::Lib(LE::MapNotFound(_)) => (304, S::BAD_REQUEST),
            E::UnknownRole(_, _) => (305, S::INTERNAL_SERVER_ERROR),
            E::UnknownRatingKind(_, _) => (307, S::INTERNAL_SERVER_ERROR),
            E::NoRatingFound(_, _) => (308, S::BAD_REQUEST),
            E::InvalidRates => (309, S::BAD_REQUEST),
            E::Lib(LE::EventNotFound(_)) => (310, S::BAD_REQUEST),
            E::Lib(LE::EventEditionNotFound(_, _)) => (311, S::BAD_REQUEST),
            E::Lib(LE::MapNotInEventEdition(_, _, _)) => (312, S::BAD_REQUEST),
            E::InvalidTimes => (313, S::BAD_REQUEST),
            E::InvalidMappackId(_) => (314, S::BAD_REQUEST),
            E::EventHasExpired(_, _) => (315, S::BAD_REQUEST),
        }
    }
}

impl From<DbErr> for RecordsErrorKind {
    fn from(value: DbErr) -> Self {
        Self::Lib(value.into())
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
pub struct TracedError {
    pub status_code: Option<StatusCode>,
    pub r#type: Option<i32>,
    pub request_id: RequestId,
    pub error: actix_web::Error,
}

impl fmt::Display for TracedError {
    #[inline(always)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl std::error::Error for TracedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl TracedError {
    fn to_err_res(&self, r#type: i32) -> ErrorResponse {
        ErrorResponse {
            request_id: self.request_id.to_string(),
            r#type,
            message: self.error.to_string(),
        }
    }
}

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    pub request_id: String,
    pub r#type: i32,
    pub message: String,
}

impl actix_web::ResponseError for TracedError {
    fn error_response(&self) -> HttpResponse {
        let r#type = self.r#type.unwrap_or(105);
        let status_code = self
            .status_code
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        HttpResponse::build(status_code).json(self.to_err_res(r#type))
    }
}

pub type RecordsResult<T> = Result<T, RecordsErrorKind>;

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
