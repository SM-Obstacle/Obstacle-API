//! The main crate of the ShootMania Obstacle API infrastructure.
//!
//! This crate is used by all the services related to the API. It contains environment setup
//! functions, the models saved in the database, and some other stuff.
//!
//! If you wish to see the crate of the server program itself, take a look
//! at the [`game_api`] package.

#![warn(missing_docs)]

use deadpool_redis::Runtime;
use once_cell::sync::OnceCell;
use sqlx::{MySql, Pool};
use std::time::Duration;

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

/// Represents the database of the API, meaning the MySQL and Redis pools.
#[derive(Clone)]
pub struct Database {
    /// The MySQL (more precisely MariaDB) pool.
    pub mysql_pool: MySqlPool,
    /// The Redis pool.
    pub redis_pool: RedisPool,
}

mkenv::make_env! {
/// The environment used to set up a connection to the MySQL/MariaDB database.
pub DbUrlEnv:
    /// The database URL.
    #[cfg(debug_assertions)]
    db_url: {
        id: DbUrl(String),
        kind: normal,
        var: "DATABASE_URL",
        desc: "The URL to the MySQL/MariaDB database",
    },
    /// The path to the file containing the database URL.
    #[cfg(not(debug_assertions))]
    db_url: {
        id: DbUrl(String),
        kind: file,
        var: "DATABASE_URL",
        desc: "The path to the file containing the URL to the MySQL/MariaDB database",
    },
}

mkenv::make_env! {
/// The environment used to set up a connection with the Redis database.
pub RedisUrlEnv:
    /// The URL to the Redis database.
    redis_url: {
        id: RedisUrl(String),
        kind: normal,
        var: "REDIS_URL",
        desc: "The URL to the Redis database",
    }
}

mkenv::make_env! {
    /// The environment used to set up a connection to the databases of the API.
    pub DbEnv includes [
        /// The environment for the MySQL/MariaDB database.
        DbUrlEnv as db_url,
        /// The environment for the Redis database.
        RedisUrlEnv as redis_url
    ]:
}

mkenv::make_env! {
/// The environment used by this crate.
pub LibEnv:
    /// The time-to-live for the mappacks saved in the Redis database.
    mappack_ttl: {
        id: MappackTtl(i64),
        kind: parse,
        var: "RECORDS_API_MAPPACK_TTL",
        desc: "The TTL (time-to-live) of the mappacks stored in Redis",
    }
}

static ENV: OnceCell<LibEnv> = OnceCell::new();

/// Initializes the provided library environment as global.
pub fn init_env(env: LibEnv) {
    ENV.set(env)
        .unwrap_or_else(|_| panic!("lib env already set"));
}

/// Returns a static reference to the global library environment.
///
/// **Caution**: To use this function, the [`init_env()`] function must have been called at the start
/// of the program.
pub fn env() -> &'static LibEnv {
    // SAFETY: this function is always called when `init_env()` is called at the start.
    unsafe { ENV.get_unchecked() }
}

/// Creates and returns the MySQL/MariaDB pool with the provided URL.
pub async fn get_mysql_pool(url: String) -> anyhow::Result<MySqlPool> {
    let mysql_pool = sqlx::mysql::MySqlPoolOptions::new()
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await?;
    Ok(mysql_pool)
}

/// Creates and returns the Redis pool with the provided URL.
pub fn get_redis_pool(url: String) -> anyhow::Result<RedisPool> {
    let cfg = deadpool_redis::Config {
        url: Some(url),
        connection: None,
        pool: None,
    };
    Ok(cfg.create_pool(Some(Runtime::Tokio1))?)
}
