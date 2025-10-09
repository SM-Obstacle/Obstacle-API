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
#[rustfmt::skip]
pub enum ApiErrorKind<E = records_lib::error::RecordsError> {
    // --------
    // --- Internal server errors
    // --------

    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error("unknown error: {0}")]
    Unknown(String),
    #[error("server is in maintenance since {0}")]
    Maintenance(chrono::NaiveDateTime),
    #[error("unknown api status: `{0}` named `{1}`")]
    UnknownStatus(u8, String),

    // --------
    // --- Authentication errors
    // --------

    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("missing the /player/get_token request")]
    MissingGetTokenReq,
    #[error("the state has already been received by the server")]
    StateAlreadyReceived(chrono::DateTime<chrono::Utc>),
    #[error("banned player")]
    BannedPlayer(banishments::Model),
    #[error("error on sending request to MP services: {0:?}")]
    AccessTokenErr(AccessTokenErr),
    #[error("invalid ManiaPlanet code on /player/give_token request")]
    InvalidMPCode,
    #[error("timeout exceeded")]
    Timeout(std::time::Duration),

    // --------
    // --- Logical errors
    // --------

    #[error("not found")]
    EndpointNotFound,
    #[error("player not banned: `{0}`")]
    PlayerNotBanned(String),
    #[error("unknown rating kind with id `{0}` and name `{1}`")]
    UnknownRatingKind(u8, String),
    #[error("no rating found to update for player with login: `{0}` and map with uid: `{1}`")]
    NoRatingFound(String, String),
    #[error("invalid rates (too many, or repeated rate)")]
    InvalidRates,
    #[error("invalid times")]
    InvalidTimes,
    #[error("event `{0}` {1} has expired")]
    EventHasExpired(String, u32),

    #[error(transparent)]
    Lib(E),
}

#[derive(serde::Serialize)]
pub struct RecordsErrorKindResponse {
    pub r#type: i32,
    pub message: String,
}

impl actix_web::ResponseError for ApiErrorKind {
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

#[macro_export]
macro_rules! internal {
    ($($t:tt)*) => {{
        $crate::ApiErrorKind::Lib($crate::__private::internal!($($t)*))
    }};
}

impl<E> ApiErrorKind<E>
where
    E: AsRef<records_lib::error::RecordsError>,
{
    pub fn get_err_type_and_status_code(&self) -> (i32, StatusCode) {
        use ApiErrorKind as E;
        use StatusCode as S;
        use records_lib::error::RecordsError as LE;

        match self {
            E::IOError(_) => (101, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::MySql(_)) => (102, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::Redis(_)) => (103, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::ExternalRequest(_)) => {
                (104, S::INTERNAL_SERVER_ERROR)
            }
            E::Unknown(_) => (105, S::INTERNAL_SERVER_ERROR),
            E::Maintenance(_) => (106, S::INTERNAL_SERVER_ERROR),
            E::UnknownStatus(_, _) => (107, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::PoolError(_)) => (108, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::Internal(_)) => (109, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::DbError(_)) => (110, S::INTERNAL_SERVER_ERROR),
            E::Lib(e) if matches!(e.as_ref(), LE::MaskedInternal) => {
                (111, S::INTERNAL_SERVER_ERROR)
            }

            E::Unauthorized => (201, S::UNAUTHORIZED),
            E::Forbidden => (202, S::FORBIDDEN),
            E::MissingGetTokenReq => (203, S::BAD_REQUEST),
            E::StateAlreadyReceived(_) => (204, S::BAD_REQUEST),
            E::BannedPlayer(_) => (205, S::FORBIDDEN),
            E::AccessTokenErr(_) => (206, S::BAD_REQUEST),
            E::InvalidMPCode => (207, S::BAD_REQUEST),
            E::Timeout(_) => (208, S::REQUEST_TIMEOUT),

            E::EndpointNotFound => (301, S::NOT_FOUND),
            E::Lib(e) if matches!(e.as_ref(), LE::PlayerNotFound(_)) => (302, S::BAD_REQUEST),
            E::PlayerNotBanned(_) => (303, S::BAD_REQUEST),
            E::Lib(e) if matches!(e.as_ref(), LE::MapNotFound(_)) => (304, S::BAD_REQUEST),
            E::Lib(e) if matches!(e.as_ref(), LE::UnknownRole(_, _)) => {
                (305, S::INTERNAL_SERVER_ERROR)
            }
            E::UnknownRatingKind(_, _) => (307, S::INTERNAL_SERVER_ERROR),
            E::NoRatingFound(_, _) => (308, S::BAD_REQUEST),
            E::InvalidRates => (309, S::BAD_REQUEST),
            E::Lib(e) if matches!(e.as_ref(), LE::EventNotFound(_)) => (310, S::BAD_REQUEST),
            E::Lib(e) if matches!(e.as_ref(), LE::EventEditionNotFound(_, _)) => {
                (311, S::BAD_REQUEST)
            }
            E::Lib(e) if matches!(e.as_ref(), LE::MapNotInEventEdition(_, _, _)) => {
                (312, S::BAD_REQUEST)
            }
            E::InvalidTimes => (313, S::BAD_REQUEST),
            E::Lib(e) if matches!(e.as_ref(), LE::InvalidMappackId(_)) => (314, S::BAD_REQUEST),
            E::EventHasExpired(_, _) => (315, S::BAD_REQUEST),

            E::Lib(_) => (199, S::INTERNAL_SERVER_ERROR),
        }
    }
}

impl From<DbErr> for ApiErrorKind {
    fn from(value: DbErr) -> Self {
        Self::Lib(value.into())
    }
}

impl From<sqlx::Error> for ApiErrorKind {
    fn from(value: sqlx::Error) -> Self {
        Self::Lib(value.into())
    }
}

impl<T> From<SendError<T>> for ApiErrorKind {
    fn from(value: SendError<T>) -> Self {
        Self::Unknown(format!("send error: {value:?}"))
    }
}

impl From<records_lib::error::RecordsError> for ApiErrorKind {
    fn from(value: records_lib::error::RecordsError) -> Self {
        Self::Lib(value)
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

pub type RecordsResult<T> = Result<T, ApiErrorKind>;

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
