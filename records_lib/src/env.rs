use std::time::Duration;

use deadpool_redis::Runtime;
use once_cell::sync::OnceCell;

use crate::{MySqlPool, RedisPool};

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

const DEFAULT_MAPPACK_TTL: i64 = 604_800;

mkenv::make_env! {
/// The environment used by this crate.
pub LibEnv:
    /// The time-to-live for the mappacks saved in the Redis database.
    mappack_ttl: {
        id: MappackTtl(i64),
        kind: parse,
        var: "RECORDS_API_MAPPACK_TTL",
        desc: "The TTL (time-to-live) of the mappacks stored in Redis",
        default: DEFAULT_MAPPACK_TTL,
    }
}

static ENV: OnceCell<LibEnv> = OnceCell::new();

/// Initializes the provided library environment as global.
///
/// If this function has already been called, the provided environment will be ignored.
pub fn init_env(env: LibEnv) {
    let _ = ENV.set(env);
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
        .max_connections(100)
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
