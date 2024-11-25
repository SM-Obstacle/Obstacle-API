//! The main crate of the ShootMania Obstacle API infrastructure.
//!
//! This crate is used by all the services related to the API. It contains environment setup
//! functions, the models saved in the database, and some other stuff.
//!
//! If you wish to see the crate of the server program itself, take a look
//! at the [`game_api`](../game_api/index.html) package.

#![warn(missing_docs)]

mod env;
mod modeversion;
mod mptypes;

pub mod context;
pub mod error;
pub mod event;
pub mod leaderboard;
pub mod map;
pub mod mappack;
pub mod models;
pub mod must;
pub mod player;
pub mod ranks;
pub mod redis_key;
pub mod table_lock;
pub mod time;

/// The MySQL/MariaDB pool type.
pub type MySqlPool = sqlx::Pool<sqlx::MySql>;
/// The Redis pool type.
pub type RedisPool = deadpool_redis::Pool;
/// The type of a Redis connection.
pub type RedisConnection = deadpool_redis::Connection;

pub use env::*;
pub use modeversion::*;
pub use mptypes::*;

/// Represents a connection to the API database, both MariaDB and Redis.
pub struct DatabaseConnection<'a> {
    /// The connection to the MariaDB database.
    pub mysql_conn: &'a mut sqlx::pool::PoolConnection<sqlx::MySql>,
    /// The connection to the Redis database.
    pub redis_conn: &'a mut RedisConnection,
}

/// Represents the database of the API, meaning the MariaDB and Redis pools.
#[derive(Clone)]
pub struct Database {
    /// The MySQL (more precisely MariaDB) pool.
    pub mysql_pool: MySqlPool,
    /// The Redis pool.
    pub redis_pool: RedisPool,
}

// TODO: remove this after doing the tests
#[allow(missing_docs)]
#[macro_export]
macro_rules! acquire {
    ($db:ident $($t:tt)*) => {{
        $crate::DatabaseConnection {
            mysql_conn: &mut $db.mysql_pool.acquire().await $($t)*,
            redis_conn: &mut $db.redis_pool.get().await $($t)*,
        }
    }};
}
