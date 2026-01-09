use graphql_api::config::InitError;
use mkenv::{error::ConfigInitError, prelude::*};
use once_cell::sync::OnceCell;
use records_lib::{DbEnv, LibEnv};

#[cfg(not(debug_assertions))]
mkenv::make_config! {
    pub struct DynamicApiEnv {
        pub sess_key: {
            var_name: "RECORDS_API_SESSION_KEY_FILE",
            layers: [file_read()],
            description: "The path to the file containing the session key used by the API",
        },

        pub mp_client_id: {
            var_name: "RECORDS_MP_APP_CLIENT_ID_FILE",
            layers: [file_read()],
            description: "The path to the file containing the Obstacle ManiaPlanet client ID",
        },

        pub mp_client_secret: {
            var_name: "RECORDS_MP_APP_CLIENT_SECRET_FILE",
            layers: [file_read()],
            description: "The path to the file containing the Obstacle ManiaPlanet client secret",
        },
    }
}

#[cfg(debug_assertions)]
mkenv::make_config! {
    pub struct DynamicApiEnv {
        pub sess_key: {
            var_name: "RECORDS_API_SESSION_KEY",
            layers: [or_default()],
            description: "The session key used by the API",
            default_val_fmt: "empty",
        },

        pub mp_client_id: {
            var_name: "RECORDS_MP_APP_CLIENT_ID",
            layers: [or_default()],
            description: "The Obstacle ManiaPlanet client ID",
            default_val_fmt: "empty",
        },

        pub mp_client_secret: {
            var_name: "RECORDS_MP_APP_CLIENT_SECRET",
            layers: [or_default()],
            description: "The Obstacle ManiaPlanet client secret",
            default_val_fmt: "empty",
        },
    }
}

#[cfg(debug_assertions)]
mkenv::make_config! {
    pub struct Hostname {}
}

#[cfg(not(debug_assertions))]
mkenv::make_config! {
    pub struct Hostname {
        pub host: {
            var_name: "RECORDS_API_HOST",
            description: "The hostname of the server where the API is running (e.g. https://obstacle.titlepack.io)",
        }
    }
}

mkenv::make_config! {
    pub struct ApiEnv {
        pub db_env: { DbEnv },

        pub dynamic: { DynamicApiEnv },

        pub wh_invalid_req_url: {
            var_name: "WEBHOOK_INVALID_REQ_URL",
            layers: [or_default()],
            description: "The URL to the Discord webhook used to flag invalid requests",
            default_val_fmt: "empty",
        },

        pub port: {
            var_name: "RECORDS_API_PORT",
            layers: [
                parsed_from_str<u16>(),
                or_default(),
            ],
            description: "The port used to expose the API",
            default_val_fmt: "3000",
        },

        pub host: { Hostname },

        pub auth_token_ttl: {
            var_name: "RECORDS_API_TOKEN_TTL",
            layers: [
                parsed_from_str<u32>(),
                or_default_val(|| 180 * 24 * 3600),
            ],
            description: "The TTL (time-to-live) of an authentication token or anything related to it (in seconds)",
            default_val_fmt: "180 days",
        },

        pub wh_report_url: {
            var_name: "WEBHOOK_REPORT_URL",
            layers: [or_default()],
            description: "The URL to the Discord webhook used to report errors",
            default_val_fmt: "empty",
        },

        pub wh_ac_url: {
            var_name: "WEBHOOK_AC_URL",
            layers: [or_default()],
            description: "The URL to the Discord webhook used to share in-game statistics",
            default_val_fmt: "empty",
        },


        pub gql_endpoint: {
            var_name: "GQL_ENDPOINT",
            layers: [
                or_default_val(|| "/graphql".to_owned()),
            ],
            description: "The route to the GraphQL endpoint (e.g. /graphql)",
            default_val_fmt: "/graphql",
        },

        pub wh_rank_compute_err: {
            var_name: "WEBHOOK_RANK_COMPUTE_ERROR",
            layers: [or_default()],
            description: "The URL to the Discord webhook used to send rank compute errors",
            default_val_fmt: "empty",
        },
    }
}

static ENV: OnceCell<ApiEnv> = OnceCell::new();

pub fn env() -> &'static ApiEnv {
    ENV.get().unwrap()
}

pub fn init_env() -> anyhow::Result<()> {
    fn map_err(err: ConfigInitError<'_>) -> anyhow::Error {
        anyhow::anyhow!("{err}")
    }

    let env = ApiEnv::define();
    let lib_env = LibEnv::define();
    env.try_init().map_err(map_err)?;
    lib_env.try_init().map_err(map_err)?;
    records_lib::init_env(lib_env);
    match graphql_api::init_config() {
        Ok(_) | Err(InitError::ConfigAlreadySet) => (),
        Err(InitError::Config(e)) => anyhow::bail!("{e}"),
    }
    let _ = ENV.set(env);

    Ok(())
}
