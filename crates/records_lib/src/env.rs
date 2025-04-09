use std::time::Duration;

use deadpool_redis::Runtime;
use once_cell::sync::OnceCell;

use crate::{MySqlPool, RedisPool, models};

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

// In game default parameter values

const DEFAULT_INGAME_SUBTITLE_ON_NEWLINE: bool = false;

const DEFAULT_INGAME_TITLES_POS: models::InGamePosition = models::InGamePosition::Left;
const DEFAULT_INGAME_LB_LINK_POS: models::InGamePosition = models::InGamePosition::Left;
const DEFAULT_INGAME_AUTHORS_POS: models::InGamePosition = models::InGamePosition::Right;

const DEFAULT_INGAME_TITLES_POS_X: f64 = 181.;
const DEFAULT_INGAME_TITLES_POS_Y: f64 = -29.5;

const DEFAULT_INGAME_LB_LINK_POS_X: f64 = 181.;
const DEFAULT_INGAME_LB_LINK_POS_Y: f64 = -29.5;

const DEFAULT_INGAME_AUTHORS_POS_X: f64 = 181.;
const DEFAULT_INGAME_AUTHORS_POS_Y: f64 = -29.5;

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
    },

    /// The default position of the titles of an event edition in the Titlepack menu.
    ingame_default_titles_pos: {
        id: InGameDefaultTitlesPos(models::InGamePosition),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_TITLES_POS",
        desc: "The default position (either L for left or R for right) of the titles of \
            an event edition in the Titlepack menu",
        default: DEFAULT_INGAME_TITLES_POS,
    },

    /// The default position of an event edition title in the Titlepack menu.
    ingame_default_lb_link_pos: {
        id: InGameDefaultLbLinkPos(models::InGamePosition),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_LB_LINK_POS",
        desc: "The default position (either L for left or R for right) of the leaderboards link of \
            an event edition in the Titlepack menu",
        default: DEFAULT_INGAME_LB_LINK_POS,
    },

    /// The default position of an event edition title in the Titlepack menu.
    ingame_default_authors_pos: {
        id: InGameDefaultAuthorsPos(models::InGamePosition),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_AUTHORS_POS",
        desc: "The default position (either L for left or R for right) of the author list of \
            an event edition in the Titlepack menu",
        default: DEFAULT_INGAME_AUTHORS_POS,
    },

    /// The default position in X axis of the titles of an event edition in the Titlepack menu.
    ingame_default_titles_pos_x: {
        id: InGameDefaultTitlesPosX(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_TITLES_POS_X",
        desc: "The default position in X axis of the titles of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_TITLES_POS_X,
    },

    /// The default position in Y axis of the titles of an event edition in the Titlepack menu.
    ingame_default_titles_pos_y: {
        id: InGameDefaultTitlesPosY(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_TITLES_POS_Y",
        desc: "The default position in Y axis of the titles of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_TITLES_POS_Y,
    },

    /// The default value of the boolean related to either to put the subtitle of an event edition
    /// on a new line or not, in the Titlepack menu.
    ingame_default_subtitle_on_newline: {
        id: InGameDefaultSubtitleOnNewLine(bool),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_SUBTITLE_ON_NEWLINE",
        desc: "The default value of the boolean related to either to put the subtitle of an \
            event edition on a new line or not in the Titlepack menu (boolean)",
        default: DEFAULT_INGAME_SUBTITLE_ON_NEWLINE,
    },

    /// The default position in X axis of the leaderboards link of an event edition in the Titlepack menu.
    ingame_default_lb_link_pos_x: {
        id: InGameDefaultLbLinkPosX(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_LB_LINK_POS_X",
        desc: "The default position in X axis of the leaderboards link of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_LB_LINK_POS_X,
    },

    /// The default position in Y axis of the leaderboards link of an event edition in the Titlepack menu.
    ingame_default_lb_link_pos_y: {
        id: InGameDefaultLbLinkPosY(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_LB_LINK_POS_Y",
        desc: "The default position in Y axis of the leaderboards link of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_LB_LINK_POS_Y,
    },

    /// The default position in X axis of the author list of an event edition in the Titlepack menu.
    ingame_default_authors_pos_x: {
        id: InGameDefaultAuthorsPosX(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_AUTHORS_POS_X",
        desc: "The default position in X axis of the author list of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_AUTHORS_POS_X,
    },

    /// The default position in Y axis of the author list of an event edition in the Titlepack menu.
    ingame_default_authors_pos_y: {
        id: InGameDefaultAuthorsPosY(f64),
        kind: parse,
        var: "RECORDS_API_INGAME_DEFAULT_AUTHORS_POS_Y",
        desc: "The default position in Y axis of the author list of an event edition in \
            the Titlepack menu (double)",
        default: DEFAULT_INGAME_AUTHORS_POS_Y,
    },
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
        .max_connections(20)
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
