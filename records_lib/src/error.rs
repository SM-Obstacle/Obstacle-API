//! A module containing the [`RecordsError`] struct, which contains various basic error types.

use deadpool_redis::PoolError;

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
#[repr(i32)]
#[rustfmt::skip]
pub enum RecordsError {
    // Caution: when creating a new error, you must ensure its code isn't
    // in conflict with another one in `game_api::RecordsErrorKind`.

    // --------
    // --- Internal server errors
    // --------

    /// An error that happened when interacting with the MySQL/MariaDB database.
    #[error(transparent)]
    MySql(#[from] sqlx::Error) = 102,
    /// An error that happened when interacting with the Redis database.
    #[error(transparent)]
    Redis(#[from] deadpool_redis::redis::RedisError) = 103,
    /// An error that happened when sending an external request.
    #[error(transparent)]
    ExternalRequest(#[from] reqwest::Error) = 104,
    /// An error that happened when using the Redis pool.
    #[error(transparent)]
    PoolError(#[from] PoolError) = 108,

    // --------
    // --- Logical errors
    // --------

    /// The player with the provided login was not found.
    #[error("player not found in database: `{0}`")]
    PlayerNotFound(
        /// The player login.
        String,
    ) = 302,
    /// The map with the provided UID was not found.
    #[error("map not found in database")]
    MapNotFound(
        /// The map UID.
        String,
    ) = 304,
    /// The event with the provided handle was not found.
    #[error("event `{0}` not found")]
    EventNotFound(
        /// The event handle.
        String,
    ) = 310,
    /// The event edition with the provided handle and edition ID was not found.
    #[error("event edition `{1}` not found for event `{0}`")]
    EventEditionNotFound(
        /// The event handle.
        String,
        /// The event edition ID.
        u32,
    ) = 311,
    /// The map isn't present in the provided event edition.
    #[error("map with uid `{0}` is not registered for event `{1}` edition {2}")]
    MapNotInEventEdition(
        /// The map UID.
        String,
        /// The event handle.
        String,
        /// The event edition ID.
        u32,
    ) = 312,
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
