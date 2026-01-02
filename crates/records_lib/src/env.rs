use std::time::Duration;

use entity::types::InGameAlignment;
use once_cell::sync::OnceCell;

#[cfg(debug_assertions)]
mkenv::make_config! {
    /// The environment used to set up a connection to the MySQL/MariaDB database.
    pub struct DbUrlEnv {
        /// The database URL.
        pub db_url: {
            var_name: "DATABASE_URL",
            description: "The URL to the MySQL/MariaDB database",
        }
    }
}
#[cfg(not(debug_assertions))]
mkenv::make_config! {
    /// The environment used to set up a connection to the MySQL/MariaDB database.
    pub struct DbUrlEnv {
        /// The path to the file containing the database URL.
        pub db_url: {
            var_name: "DATABASE_URL",
            layers: [
                file_read(),
            ],
            description: "The path to the file containing the URL to the MySQL/MariaDB database",
        }
    }
}

mkenv::make_config! {
    /// The environment used to set up a connection with the Redis database.
    pub struct RedisUrlEnv {
        /// The URL to the Redis database.
        pub redis_url: {
            var_name: "REDIS_URL",
            description: "The URL to the Redis database",
        }
    }
}

mkenv::make_config! {
    /// The environment used to set up a connection to the databases of the API.
    pub struct DbEnv {
        /// The environment for the MySQL/MariaDB database.
        pub db_url: { DbUrlEnv },
        /// The environment for the Redis database.
        pub redis_url: { RedisUrlEnv },
    }
}

mkenv::make_config! {
    /// The environment used by this crate.
    pub struct LibEnv {
        /// The time-to-live for the mappacks saved in the Redis database.
        pub mappack_ttl: {
            var_name: "RECORDS_API_MAPPACK_TTL",
            layers: [
                parsed_from_str<i64>(),
                or_default_val(|| 604_800),
            ],
            description: "The TTL (time-to-live) of the mappacks stored in Redis",
            default_val_fmt: "604,800",
        },

        /// The default alignment of the titles of an event edition in the Titlepack menu.
        pub ingame_default_titles_align: {
            var_name: "RECORDS_API_INGAME_DEFAULT_TITLES_ALIGN",
            layers: [
                parsed_from_str<InGameAlignment>(),
                or_default_val(|| InGameAlignment::Left),
            ],
            description: "The default alignment (either L for left or R for right) of the titles of \
                an event edition in the Titlepack menu",
            default_val_fmt: "left",
        },

        /// The default alignment of an event edition title in the Titlepack menu.
        pub ingame_default_lb_link_align: {
            var_name: "RECORDS_API_INGAME_DEFAULT_LB_LINK_ALIGN",
            layers: [
                parsed_from_str<InGameAlignment>(),
                or_default_val(|| InGameAlignment::Left),
            ],
            description: "The default alignment (either L for left or R for right) of the leaderboards link of \
                an event edition in the Titlepack menu",
            default_val_fmt: "left",
        },


        /// The default alignment of an event edition title in the Titlepack menu.
        pub ingame_default_authors_align: {
            var_name: "RECORDS_API_INGAME_DEFAULT_AUTHORS_ALIGN",
            layers: [
                parsed_from_str<InGameAlignment>(),
                or_default_val(|| InGameAlignment::Right),
            ],
            description: "The default alignment (either L for left or R for right) of the author list of \
                an event edition in the Titlepack menu",
            default_val_fmt: "right",
        },

        /// The default position in X axis of the titles of an event edition in the Titlepack menu.
        pub ingame_default_titles_pos_x: {
            var_name: "RECORDS_API_INGAME_DEFAULT_TITLES_POS_X",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| 181.),
            ],
            description: "The default position in X axis of the titles of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "-181.0",
        },

        /// The default position in Y axis of the titles of an event edition in the Titlepack menu.
        pub ingame_default_titles_pos_y: {
            var_name: "RECORDS_API_INGAME_DEFAULT_TITLES_POS_Y",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| -29.5),
            ],
            description: "The default position in Y axis of the titles of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "-29.5",
        },

        /// The default value of the boolean related to either to put the subtitle of an event edition
        /// on a new line or not, in the Titlepack menu.
        pub ingame_default_subtitle_on_newline: {
            var_name: "RECORDS_API_INGAME_DEFAULT_SUBTITLE_ON_NEWLINE",
            layers: [
                parsed_from_str<bool>(),
                or_default_val(|| false)
            ],
            description: "The default value of the boolean related to either to put the subtitle of an \
                event edition on a new line or not in the Titlepack menu (boolean)",
            default_val_fmt: "false",
        },

        /// The default position in X axis of the leaderboards link of an event edition in the Titlepack menu.
        pub ingame_default_lb_link_pos_x: {
            var_name: "RECORDS_API_INGAME_DEFAULT_LB_LINK_POS_X",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| 181.),
            ],
            description: "The default position in X axis of the leaderboards link of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "181.0",
        },

        /// The default position in Y axis of the leaderboards link of an event edition in the Titlepack menu.
        pub ingame_default_lb_link_pos_y: {
            var_name: "RECORDS_API_INGAME_DEFAULT_LB_LINK_POS_Y",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| -29.5),
            ],
            description: "The default position in Y axis of the leaderboards link of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "-29.5",
        },

        /// The default position in X axis of the author list of an event edition in the Titlepack menu.
        pub ingame_default_authors_pos_x: {
            var_name: "RECORDS_API_INGAME_DEFAULT_AUTHORS_POS_X",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| 181.),
            ],
            description: "The default position in X axis of the author list of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "181.0",
        },

        /// The default position in Y axis of the author list of an event edition in the Titlepack menu.
        pub ingame_default_authors_pos_y: {
            var_name: "RECORDS_API_INGAME_DEFAULT_AUTHORS_POS_Y",
            layers: [
                parsed_from_str<f64>(),
                or_default_val(|| -29.5),
            ],
            description: "The default position in Y axis of the author list of an event edition in \
                the Titlepack menu (double)",
            default_val_fmt: "-29.5",
        },

        /// The interval of a mappack scores update
        pub event_scores_interval: {
            var_name: "EVENT_SCORES_INTERVAL_SECONDS",
            layers: [
                parsed<Duration>(|input| {
                    input.parse().map(Duration::from_secs).map_err(From::from)
                }),
                or_default_val(|| Duration::from_secs(6 * 3600)),
            ],
            description: "The interval of the update of the event scores, in seconds",
            default_val_fmt: "6h",
        },

        /// The interval of the player/map ranking update
        pub player_map_ranking_scores_interval: {
            var_name: "PLAYER_MAP_RANKING_SCORES_INTERVAL",
            layers: [
                parsed<Duration>(|input| {
                    input.parse().map(Duration::from_secs).map_err(From::from)
                }),
                or_default_val(|| Duration::from_secs(3600 * 24 * 7)),
            ],
            description: "The interval of the update of the player/map ranking scores, in seconds",
            default_val_fmt: "every week",
        }
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
    ENV.get().unwrap()
}
