use std::time::Duration;

use deadpool_redis::Runtime;
use once_cell::sync::OnceCell;
use sqlx::{MySql, Pool};

pub mod error;
pub mod escaped;
pub mod models;
pub mod must;
pub mod redis_key;
pub mod update_mappacks;
pub mod update_ranks;

pub mod event;
pub mod map;
pub mod player;

pub type MySqlPool = Pool<MySql>;
pub type RedisPool = deadpool_redis::Pool;
pub type RedisConnection = deadpool_redis::Connection;

#[derive(Clone)]
pub struct Database {
    pub mysql_pool: MySqlPool,
    pub redis_pool: RedisPool,
}

mkenv::make_env! {pub DbUrlEnv:
    #[cfg(debug_assertions)]
    db_url: {
        id: DbUrl(String),
        kind: normal,
        var: "DATABASE_URL",
        desc: "The URL to the MySQL/MariaDB database",
    },
    #[cfg(not(debug_assertions))]
    db_url: {
        id: DbUrl(String),
        kind: file,
        var: "DATABASE_URL",
        desc: "The URL to the MySQL/MariaDB database",
    },
}

mkenv::make_env! {pub RedisUrlEnv:
    redis_url: {
        id: RedisUrl(String),
        kind: normal,
        var: "REDIS_URL",
        desc: "The URL to the Redis database",
    }
}

mkenv::make_env!(pub DbEnv includes [DbUrlEnv as db_url, RedisUrlEnv as redis_url]:);

mkenv::make_env! {pub LibEnv:
    mappack_ttl: {
        id: MappackTtl(i64),
        kind: parse,
        var: "RECORDS_API_MAPPACK_TTL",
        desc: "The TTL (time-to-live) of the mappacks stored in Redis",
    }
}

static ENV: OnceCell<LibEnv> = OnceCell::new();

pub fn init_env(env: LibEnv) {
    ENV.set(env)
        .unwrap_or_else(|_| panic!("lib env already set"));
}

pub fn env() -> &'static LibEnv {
    unsafe { ENV.get_unchecked() }
}

pub async fn get_mysql_pool(url: String) -> anyhow::Result<MySqlPool> {
    let mysql_pool = sqlx::mysql::MySqlPoolOptions::new()
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await?;
    Ok(mysql_pool)
}

pub fn get_redis_pool(url: String) -> anyhow::Result<RedisPool> {
    let cfg = deadpool_redis::Config {
        url: Some(url),
        connection: None,
        pool: None,
    };
    Ok(cfg.create_pool(Some(Runtime::Tokio1))?)
}
