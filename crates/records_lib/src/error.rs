//! A module containing the [`RecordsError`] struct, which contains various basic error types.

use deadpool_redis::PoolError;
use sea_orm::TransactionError;

/// Represents any type of error that could happen when using this crate.
///
/// To be handled by the ShootMania Obstacle gamemode, each kind of error has an associated code.
///
/// * `100..=199` errors represent internal server errors.
/// * `200..=299` errors represent authentication errors.
/// * `300..=399` errors represent logical errors (for example, an invalid player login).
///
/// This semantic is only used by the `game_api` crate when returning errors. It also has its own
/// error types that are not present here, with their associated code. These codes should not be
/// in conflict.
#[derive(thiserror::Error, Debug)]
#[rustfmt::skip]
pub enum RecordsError {
    // --------
    // --- Internal server errors
    // --------

    /// An error that happened when interacting with the MySQL/MariaDB database.
    #[error(transparent)]
    MySql(#[from] sqlx::Error),
    /// An error that happened when interacting with the Redis database.
    #[error(transparent)]
    Redis(#[from] deadpool_redis::redis::RedisError),
    /// An error that happened when sending an external request.
    #[error(transparent)]
    ExternalRequest(#[from] reqwest::Error),
    /// An error that happened when using the Redis pool.
    #[error(transparent)]
    PoolError(#[from] PoolError),
    /// An internal error.
    #[error("internal error: {0}")]
    Internal(String),
    /// A masked internal error.
    #[error("internal error")]
    MaskedInternal,
    /// An error from the database.
    #[error(transparent)]
    DbError(#[from] sea_orm::DbErr),

    // --------
    // --- Logical errors
    // --------

    /// The player with the provided login was not found.
    #[error("player with login `{0}` not found in database")]
    PlayerNotFound(
        /// The player login.
        String,
    ),
    /// The map with the provided UID was not found.
    #[error("map with uid `{0}` not found in database")]
    MapNotFound(
        /// The map UID.
        String,
    ),
    /// The event with the provided handle was not found.
    #[error("event `{0}` not found")]
    EventNotFound(
        /// The event handle.
        String,
    ),
    /// The event edition with the provided handle and edition ID was not found.
    #[error("event edition `{1}` not found for event `{0}`")]
    EventEditionNotFound(
        /// The event handle.
        String,
        /// The event edition ID.
        u32,
    ),
    /// The map isn't present in the provided event edition.
    #[error("map with uid `{0}` is not registered for event `{1}` edition {2}")]
    MapNotInEventEdition(
        /// The map UID.
        String,
        /// The event handle.
        String,
        /// The event edition ID.
        u32,
    ),
    /// Parsing error for the ID of a mappack.
    #[error("mappack id should be an integer, got `{0}`")]
    InvalidMappackId(String),
    /// The provided player role is unknown.
    #[error("unknown role with id `{0}` and name `{1}`")]
    UnknownRole(u8, String),
}

/// Shortcut for creating an internal error, by formatting a message.
///
/// See [`RecordsError::Internal`].
#[macro_export]
macro_rules! internal {
    ($($t:tt)*) => {{
        $crate::error::RecordsError::Internal($crate::error::__private::format!($($t)*))
    }};
}

#[doc(hidden)]
pub mod __private {
    pub use std::format;
}

impl<E> From<TransactionError<E>> for RecordsError
where
    RecordsError: From<E>,
{
    fn from(value: TransactionError<E>) -> Self {
        match value {
            TransactionError::Connection(db_err) => From::from(db_err),
            TransactionError::Transaction(e) => From::from(e),
        }
    }
}

impl RecordsError {
    /// Returns the associated code of the error.
    ///
    /// See the [documentation](RecordsError) for more information.
    pub fn get_code(&self) -> i32 {
        // SAFETY: Self is repr(i32).
        unsafe { *(self as *const Self as *const _) }
    }
}

/// Represents the result of a computation that could return a [`RecordsError`].
pub type RecordsResult<T = ()> = Result<T, RecordsError>;
