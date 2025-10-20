use std::{fmt, ops::RangeInclusive, sync::Arc};

use records_lib::error::RecordsError;

#[derive(Debug)]
pub enum CursorDecodeErrorKind {
    NotBase64,
    NotUtf8,
    WrongPrefix,
    NoTimestamp,
    NoTime,
    InvalidTimestamp(i64),
}

impl fmt::Display for CursorDecodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CursorDecodeErrorKind::NotBase64 => f.write_str("not base64"),
            CursorDecodeErrorKind::NotUtf8 => f.write_str("not UTF-8"),
            CursorDecodeErrorKind::WrongPrefix => f.write_str("wrong prefix"),
            CursorDecodeErrorKind::NoTimestamp => f.write_str("no timestamp"),
            CursorDecodeErrorKind::NoTime => f.write_str("no time"),
            CursorDecodeErrorKind::InvalidTimestamp(t) => {
                f.write_str("invalid timestamp: ")?;
                fmt::Display::fmt(t, f)
            }
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
pub struct CursorRangeError {
    arg_name: &'static str,
    value: usize,
    range: RangeInclusive<usize>,
}

#[derive(Debug)]
pub enum ApiGqlErrorKind {
    Lib(RecordsError),
    CursorRange(CursorRangeError),
    CursorDecode(CursorDecodeError),
    GqlError(async_graphql::Error),
    RecordNotFound { record_id: u32 },
    MapNotFound { map_uid: String },
    PlayerNotFound { login: String },
}

#[derive(Debug, Clone)]
pub struct ApiGqlError {
    inner: Arc<ApiGqlErrorKind>,
}

impl ApiGqlError {
    pub(crate) fn from_cursor_range_error(
        arg_name: &'static str,
        expected_range: RangeInclusive<usize>,
        value: usize,
    ) -> Self {
        Self {
            inner: Arc::new(ApiGqlErrorKind::CursorRange(CursorRangeError {
                arg_name,
                value,
                range: expected_range,
            })),
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
        match &*self.inner {
            ApiGqlErrorKind::Lib(records_error) => fmt::Display::fmt(records_error, f),
            ApiGqlErrorKind::CursorRange(cursor_error) => {
                write!(
                    f,
                    "`{}` must be between {} and {} included, got {}",
                    cursor_error.arg_name,
                    cursor_error.range.start(),
                    cursor_error.range.end(),
                    cursor_error.value
                )
            }
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

impl From<ApiGqlError> for async_graphql::Error {
    #[inline(always)]
    fn from(value: ApiGqlError) -> Self {
        async_graphql::Error::new_with_source(value)
    }
}

pub type GqlResult<T, E = ApiGqlError> = Result<T, E>;
