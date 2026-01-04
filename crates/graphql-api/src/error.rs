use std::{fmt, sync::Arc};

use records_lib::error::RecordsError;
use sha2::digest::MacError;

pub(crate) fn map_gql_err(e: async_graphql::Error) -> ApiGqlError {
    match e
        .source
        .as_ref()
        .and_then(|source| source.downcast_ref::<ApiGqlError>())
    {
        Some(err) => err.clone(),
        None => ApiGqlError::from_gql_error(e),
    }
}

#[derive(Debug, PartialEq)]
pub enum CursorDecodeErrorKind {
    NotBase64,
    NotUtf8,
    WrongPrefix,
    NoTimestamp,
    NoSignature,
    InvalidSignature(MacError),
    NoTime,
    NoScore,
    InvalidTimestamp(i64),
    MissingText,
    MissingScore,
    MissingData,
    TooLong,
}

impl fmt::Display for CursorDecodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CursorDecodeErrorKind::NotBase64 => f.write_str("not base64"),
            CursorDecodeErrorKind::NotUtf8 => f.write_str("not UTF-8"),
            CursorDecodeErrorKind::WrongPrefix => f.write_str("wrong prefix"),
            CursorDecodeErrorKind::NoTimestamp => f.write_str("no timestamp"),
            CursorDecodeErrorKind::NoSignature => f.write_str("no signature"),
            CursorDecodeErrorKind::InvalidSignature(e) => write!(f, "invalid signature: {e}"),
            CursorDecodeErrorKind::NoTime => f.write_str("no time"),
            CursorDecodeErrorKind::NoScore => f.write_str("no score"),
            CursorDecodeErrorKind::InvalidTimestamp(t) => {
                f.write_str("invalid timestamp: ")?;
                fmt::Display::fmt(t, f)
            }
            CursorDecodeErrorKind::MissingText => f.write_str("missing text"),
            CursorDecodeErrorKind::MissingScore => f.write_str("missing score"),
            CursorDecodeErrorKind::MissingData => f.write_str("missing data"),
            CursorDecodeErrorKind::TooLong => f.write_str("input too long"),
        }
    }
}

#[derive(Debug)]
pub struct CursorDecodeError {
    arg_name: &'static str,
    value: String,
    kind: CursorDecodeErrorKind,
}

#[derive(Debug)]
pub enum ApiGqlErrorKind {
    Lib(RecordsError),
    CursorDecode(CursorDecodeError),
    PaginationInput,
    GqlError(async_graphql::Error),
    RecordNotFound { record_id: u32 },
    MapNotFound { map_uid: String },
    PlayerNotFound { login: String },
}

impl fmt::Display for ApiGqlErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiGqlErrorKind::Lib(records_error) => fmt::Display::fmt(records_error, f),
            ApiGqlErrorKind::PaginationInput => f.write_str(
                "cursor pagination input is invalid. \
                must provide either: `after`, `after` with `first`, \
                `before`, or `before` with `last`.",
            ),
            ApiGqlErrorKind::CursorDecode(decode_error) => {
                write!(
                    f,
                    "cursor argument `{}` couldn't be decoded: {}. got `{}`",
                    decode_error.arg_name, decode_error.kind, decode_error.value
                )
            }
            ApiGqlErrorKind::GqlError(error) => f.write_str(&error.message),
            ApiGqlErrorKind::RecordNotFound { record_id } => {
                write!(f, "record `{record_id}` not found")
            }
            ApiGqlErrorKind::MapNotFound { map_uid } => {
                write!(f, "map with UID `{map_uid}` not found")
            }
            ApiGqlErrorKind::PlayerNotFound { login } => {
                write!(f, "player with login `{login}` not found")
            }
        }
    }
}

impl std::error::Error for ApiGqlErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ApiGqlErrorKind::Lib(records_error) => Some(records_error),
            ApiGqlErrorKind::CursorDecode(_) => None,
            ApiGqlErrorKind::PaginationInput => None,
            ApiGqlErrorKind::GqlError(_) => None,
            ApiGqlErrorKind::RecordNotFound { .. } => None,
            ApiGqlErrorKind::MapNotFound { .. } => None,
            ApiGqlErrorKind::PlayerNotFound { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiGqlError {
    inner: Arc<ApiGqlErrorKind>,
}

impl std::error::Error for ApiGqlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

impl ApiGqlError {
    pub(crate) fn from_pagination_input_error() -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::PaginationInput),
        }
    }

    pub(crate) fn from_cursor_decode_error(
        arg_name: &'static str,
        value: String,
        decode_error: CursorDecodeErrorKind,
    ) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::CursorDecode(CursorDecodeError {
                arg_name,
                value,
                kind: decode_error,
            })),
        }
    }

    pub(crate) fn from_gql_error(error: async_graphql::Error) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::GqlError(error)),
        }
    }

    pub(crate) fn from_record_not_found_error(record_id: u32) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::RecordNotFound { record_id }),
        }
    }

    pub(crate) fn from_map_not_found_error(map_uid: String) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::MapNotFound { map_uid }),
        }
    }

    pub(crate) fn from_player_not_found_error(login: String) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::PlayerNotFound { login }),
        }
    }
}

impl ApiGqlError {
    pub fn kind(&self) -> &ApiGqlErrorKind {
        &self.inner
    }
}

impl<E> From<E> for ApiGqlError
where
    RecordsError: From<E>,
{
    fn from(value: E) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::Lib(From::from(value))),
        }
    }
}

impl fmt::Display for ApiGqlError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

impl From<ApiGqlError> for async_graphql::Error {
    #[inline(always)]
    fn from(value: ApiGqlError) -> Self {
        async_graphql::Error::new_with_source(value)
    }
}

pub type GqlResult<T> = Result<T, ApiGqlError>;
