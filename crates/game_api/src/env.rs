use mkenv::{Env as _, EnvSplitIncluded as _};
use once_cell::sync::OnceCell;
use records_lib::{DbEnv, LibEnv};

#[cfg(feature = "test")]
const DEFAULT_SESSION_KEY: &str = "";

mkenv::make_env! {pub ApiEnvUsedOnce:
    #[cfg(not(feature = "test"))]
    sess_key: {
        id: SessKey(String),
        kind: file,
        var: "RECORDS_API_SESSION_KEY_FILE",
        desc: "The path to the file containing the session key used by the API",
    },

    #[cfg(feature = "test")]
    sess_key: {
        id: SessKey(String),
        kind: normal,
        var: "RECORDS_API_SESSION_KEY",
        desc: "The session key used by the API",
        default: DEFAULT_SESSION_KEY,
    },

    wh_invalid_req_url: {
        id: WebhookInvalidReqUrl(String),
        kind: normal,
        var: "WEBHOOK_INVALID_REQ_URL",
        desc: "The URL to the Discord webhook used to flag invalid requests",
        default: DEFAULT_WH_INVALID_REQ_URL,
    },
}

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_TOKEN_TTL: u32 = 15_552_000;
#[cfg(feature = "test")]
const DEFAULT_MP_CLIENT_ID: &str = "";
#[cfg(feature = "test")]
const DEFAULT_MP_CLIENT_SECRET: &str = "";
const DEFAULT_WH_REPORT_URL: &str = "";
const DEFAULT_WH_AC_URL: &str = "";
const DEFAULT_WH_INVALID_REQ_URL: &str = "";
const DEFAULT_GQL_ENDPOINT: &str = "/graphql";

mkenv::make_env! {pub ApiEnv includes [
    DbEnv as db_env,
    LibEnv as lib_env,
    ApiEnvUsedOnce as used_once
]:
    port: {
        id: Port(u16),
        kind: parse,
        var: "RECORDS_API_PORT",
        desc: "The port used to expose the API",
        default: DEFAULT_PORT,
    },

    #[cfg(not(debug_assertions))]
    host: {
        id: Host(String),
        kind: normal,
        var: "RECORDS_API_HOST",
        desc: "The hostname of the server where the API is running (e.g. https://obstacle.titlepack.io)",
    },

    auth_token_ttl: {
        id: AuthTokenTtl(u32),
        kind: parse,
        var: "RECORDS_API_TOKEN_TTL",
        desc: "The TTL (time-to-live) of an authentication token or anything related to it (in seconds)",
        default: DEFAULT_TOKEN_TTL,
    },

    #[cfg(not(feature = "test"))]
    mp_client_id: {
        id: MpClientId(String),
        kind: file,
        var: "RECORDS_MP_APP_CLIENT_ID_FILE",
        desc: "The path to the file containing the Obstacle ManiaPlanet client ID",
    },
    #[cfg(feature = "test")]
    mp_client_id: {
        id: MpClientId(String),
        kind: normal,
        var: "RECORDS_MP_APP_CLIENT_ID",
        desc: "The Obstacle ManiaPlanet client ID",
        default: DEFAULT_MP_CLIENT_ID,
    },

    #[cfg(not(feature = "test"))]
    mp_client_secret: {
        id: MpClientSecret(String),
        kind: file,
        var: "RECORDS_MP_APP_CLIENT_SECRET_FILE",
        desc: "The path to the file containing the Obstacle ManiaPlanet client secret",
    },
    #[cfg(feature = "test")]
    mp_client_secret: {
        id: MpClientSecret(String),
        kind: normal,
        var: "RECORDS_MP_APP_CLIENT_SECRET",
        desc: "The Obstacle ManiaPlanet client secret",
        default: DEFAULT_MP_CLIENT_SECRET,
    },

    wh_report_url: {
        id: WebhookReportUrl(String),
        kind: normal,
        var: "WEBHOOK_REPORT_URL",
        desc: "The URL to the Discord webhook used to report errors",
        default: DEFAULT_WH_REPORT_URL,
    },

    wh_ac_url: {
        id: WebhookAcUrl(String),
        kind: normal,
        var: "WEBHOOK_AC_URL",
        desc: "The URL to the Discord webhook used to share in-game statistics",
        default: DEFAULT_WH_AC_URL,
    },

    gql_endpoint: {
        id: GqlEndpoint(String),
        kind: normal,
        var: "GQL_ENDPOINT",
        desc: "The route to the GraphQL endpoint (e.g. /graphql)",
        default: DEFAULT_GQL_ENDPOINT,
    },
}

pub struct InitEnvOut {
    pub db_env: DbEnv,
    pub used_once: ApiEnvUsedOnce,
}

static ENV: OnceCell<mkenv::init_env!(ApiEnv)> = OnceCell::new();

pub fn env() -> &'static mkenv::init_env!(ApiEnv) {
    // SAFETY: this function is always called when the `init_env()` is called at the start.
    unsafe { ENV.get_unchecked() }
}

pub fn init_env() -> anyhow::Result<InitEnvOut> {
    let env = ApiEnv::try_get()?;
    let (included, rest) = env.split();
    records_lib::init_env(included.lib_env);
    let _ = ENV.set(rest);

    Ok(InitEnvOut {
        db_env: included.db_env,
        used_once: included.used_once,
    })
}
