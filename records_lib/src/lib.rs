//! The main crate of the ShootMania Obstacle API infrastructure.
//!
//! This crate is used by all the services related to the API. It contains environment setup
//! functions, the models saved in the database, and some other stuff.
//!
//! If you wish to see the crate of the server program itself, take a look
//! at the [`game_api`](../game_api/index.html) package.

#![warn(missing_docs)]

use sqlx::{pool::PoolConnection, MySql, Pool};

mod env;
mod modeversion;
mod mptypes;

pub mod error;
pub mod mappack;
pub mod models;
pub mod must;
pub mod redis_key;
pub mod update_ranks;

pub mod event;
pub mod map;
pub mod player;

/// The MySQL/MariaDB pool type.
pub type MySqlPool = Pool<MySql>;
/// The Redis pool type.
pub type RedisPool = deadpool_redis::Pool;
/// The type of a Redis connection.
pub type RedisConnection = deadpool_redis::Connection;

pub use env::*;
pub use modeversion::*;
pub use mptypes::*;

use self::error::RecordsResult;

/// Represents a connection to the API database, both MariaDB and Redis.
pub struct DatabaseConnection {
    /// The connection to the MariaDB database.
    pub mysql_conn: PoolConnection<MySql>,
    /// The connection to the Redis database.
    pub redis_conn: RedisConnection,
}

/// Represents the database of the API, meaning the MariaDB and Redis pools.
#[derive(Clone)]
pub struct Database {
    /// The MySQL (more precisely MariaDB) pool.
    pub mysql_pool: MySqlPool,
    /// The Redis pool.
    pub redis_pool: RedisPool,
}

impl Database {
    /// Retrieves a connection object for each database pool, and returns them wrapped in a
    /// [`DatabaseConnection`].
    pub async fn acquire(&self) -> RecordsResult<DatabaseConnection> {
        Ok(DatabaseConnection {
            mysql_conn: self.mysql_pool.acquire().await?,
            redis_conn: self.redis_pool.get().await?,
        })
    }
}
