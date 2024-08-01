//! The ShootMania Obstacle API library that the program uses.
//!
//! The content of this library is only made for the API program.

use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use core::fmt;
pub use deadpool_redis::Pool as RedisPool;
use mkenv::{Env, EnvSplitIncluded};
use once_cell::sync::OnceCell;
use records_lib::{models, DbEnv, LibEnv};
use serde::{Deserialize, Serialize};
pub use sqlx::MySqlPool;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::{ready, Ready};
use std::io;
use std::ops::{Deref, DerefMut};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tracing_actix_web::RequestId;

mod auth;
mod graphql;
mod http;
pub(crate) mod must;
mod utils;

pub use auth::AuthState;
pub use graphql::graphql_route;
pub use http::api_route;

#[derive(Deserialize, Debug, Clone)]
#[allow(unused)] // not used in code, but displayed as debug
pub struct AccessTokenErr {
    error: String,
    message: String,
    hint: String,
}

#[derive(Error, Debug)]
#[repr(i32)] // i32 to be used with clients that don't support unsigned integers
#[rustfmt::skip]
pub enum RecordsErrorKind {
    // Caution: when creating a new error, you must ensure its code isn't
    // in conflict with another one in `records_lib::RecordsError`.

    // --------
    // --- Internal server errors
    // --------

    #[error(transparent)]
    IOError(#[from] io::Error) = 101,

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
    StateAlreadyReceived(DateTime<Utc>) = 204,
    #[error("banned player {0}")]
    BannedPlayer(models::Banishment) = 205,
    #[error("error on sending request to MP services: {0:?}")]
    AccessTokenErr(AccessTokenErr) = 206,
    #[error("invalid ManiaPlanet code on /player/give_token request")]
    InvalidMPCode = 207,
    #[error("timeout exceeded (max 5 minutes)")]
    Timeout = 208,

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
    #[error("unknown medal with id `{0}` and name `{1}`")]
    UnknownMedal(u8, String) = 306,
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

#[derive(Serialize)]
struct ErrorResponse {
    request_id: String,
    r#type: i32,
    message: String,
}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use records_lib::error::RecordsError as LR;
        use RecordsErrorKind as R;

        match &self.kind {
            // Internal server errors
            R::IOError(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Unknown(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::Maintenance(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::UnknownStatus(..) => HttpResponse::InternalServerError().json(self.to_err_res()),

            // Authentication errors
            R::Unauthorized => HttpResponse::Unauthorized().json(self.to_err_res()),
            R::Forbidden => HttpResponse::Forbidden().json(self.to_err_res()),
            R::MissingGetTokenReq => HttpResponse::BadRequest().json(self.to_err_res()),
            R::StateAlreadyReceived(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::BannedPlayer(ban) => {
                #[derive(Serialize)]
                struct BannedPlayerResponse<'a> {
                    message: String,
                    ban: &'a models::Banishment,
                }
                HttpResponse::Forbidden().json(BannedPlayerResponse {
                    message: ban.to_string(),
                    ban,
                })
            }
            R::AccessTokenErr(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidMPCode => HttpResponse::BadRequest().json(self.to_err_res()),
            R::Timeout => HttpResponse::RequestTimeout().json(self.to_err_res()),

            // Logical errors
            R::EndpointNotFound => HttpResponse::NotFound().json(self.to_err_res()),
            R::PlayerNotBanned(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::UnknownRole(..) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::UnknownMedal(..) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::UnknownRatingKind(..) => HttpResponse::InternalServerError().json(self.to_err_res()),
            R::NoRatingFound(..) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidRates => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidTimes => HttpResponse::BadRequest().json(self.to_err_res()),
            R::InvalidMappackId(_) => HttpResponse::BadRequest().json(self.to_err_res()),
            R::EventHasExpired(..) => HttpResponse::BadRequest().json(self.to_err_res()),

            R::Lib(e) => match e {
                // Internal server errors
                LR::MySql(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
                LR::Redis(_) => HttpResponse::InternalServerError().json(self.to_err_res()),
                LR::ExternalRequest(_) => {
                    HttpResponse::InternalServerError().json(self.to_err_res())
                }
                LR::PoolError(_) => HttpResponse::InternalServerError().json(self.to_err_res()),

                // Logical errors
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

/// A resource handler, like [`Data`][d].
///
/// The difference with [`Data`][d] is that it doesn't use an [`Arc`](std::sync::Arc)
/// internally, but the [`Clone`] implementation of the inner type to implement [`FromRequest`].
///
/// [d]: actix_web::web::Data
#[derive(Clone)]
pub struct Res<T>(pub T);

impl<T> From<T> for Res<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> AsRef<T> for Res<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> AsMut<T> for Res<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut *self
    }
}

impl<T> Deref for Res<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Res<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Clone + 'static> FromRequest for Res<T> {
    type Error = Infallible;

    type Future = Ready<Result<Self, Infallible>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let client = req
            .app_data::<T>()
            .unwrap_or_else(|| panic!("{} should be present", std::any::type_name::<T>()))
            .clone();
        ready(Ok(Self(client)))
    }

    fn extract(req: &HttpRequest) -> Self::Future {
        Self::from_request(req, &mut Payload::None)
    }
}

mkenv::make_env! {pub ApiEnvUsedOnce:
    sess_key: {
        id: SessKey(String),
        kind: file,
        var: "RECORDS_API_SESSION_KEY_FILE",
        desc: "The path to the file containing the session key used by the API",
    }
}

const DEFAULT_GQL_ENDPOINT: &str = "/graphql";

mkenv::make_env! {pub ApiEnv includes [
    DbEnv as db_env,
    LibEnv as lib_env,
    ApiEnvUsedOnce as used_once
]:
    port: {
        id: Port(u16),
        kind: parse,
        var: "RECORDS_API_PORT",
        desc: "The port used to expose the API",
    },

    #[cfg(not(debug_assertions))]
    host: {
        id: Host(String),
        kind: normal,
        var: "RECORDS_API_HOST",
        desc: "The hostname of the server where the API is running (e.g. https://obstacle.titlepack.io)",
    },

    auth_token_ttl: {
        id: AuthTokenTtl(u32),
        kind: parse,
        var: "RECORDS_API_TOKEN_TTL",
        desc: "The TTL (time-to-live) of an authentication token or anything related to it (in seconds)",
    },

    mp_client_id: {
        id: MpClientId(String),
        kind: file,
        var: "RECORDS_MP_APP_CLIENT_ID_FILE",
        desc: "The path to the file containing the Obstacle ManiaPlanet client ID",
    },

    mp_client_secret: {
        id: MpClientSecret(String),
        kind: file,
        var: "RECORDS_MP_APP_CLIENT_SECRET_FILE",
        desc: "The path to the file containing the Obstacle ManiaPlanet client secret",
    },

    wh_report_url: {
        id: WebhookReportUrl(String),
        kind: normal,
        var: "WEBHOOK_REPORT_URL",
        desc: "The URL to the Discord webhook used to report errors",
    },

    wh_ac_url: {
        id: WebhookAcUrl(String),
        kind: normal,
        var: "WEBHOOK_AC_URL",
        desc: "The URL to the Discord webhook used to share in-game statistics",
    },

    gql_endpoint: {
        id: GqlEndpoint(String),
        kind: normal,
        var: "GQL_ENDPOINT",
        desc: "The route to the GraphQL endpoint (e.g. /graphql)",
        default: DEFAULT_GQL_ENDPOINT,
    }
}

pub struct InitEnvOut {
    pub db_env: DbEnv,
    pub used_once: ApiEnvUsedOnce,
}

static ENV: OnceCell<mkenv::init_env!(ApiEnv)> = OnceCell::new();

pub fn env() -> &'static mkenv::init_env!(ApiEnv) {
    // SAFETY: this function is always called when the `init_env()` is called at the start.
    unsafe { ENV.get_unchecked() }
}

pub fn init_env() -> anyhow::Result<InitEnvOut> {
    let env = ApiEnv::try_get()?;
    let (included, rest) = env.split();
    records_lib::init_env(included.lib_env);
    ENV.set(rest)
        .unwrap_or_else(|_| panic!("api env already set"));

    Ok(InitEnvOut {
        db_env: included.db_env,
        used_once: included.used_once,
    })
}
