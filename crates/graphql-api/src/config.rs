use hmac::Hmac;
use sha2::Sha224;
#[cfg(debug_assertions)]
use sha2::digest::KeyInit;
use std::{error::Error, fmt, sync::OnceLock};

use mkenv::{ConfigDescriptor, exec::ConfigInitializer};

pub type SecretKeyType = Hmac<Sha224>;

fn parse_secret_key(input: &str) -> Result<SecretKeyType, Box<dyn Error>> {
    SecretKeyType::new_from_slice(input.as_bytes()).map_err(From::from)
}

#[cfg(debug_assertions)]
mkenv::make_config! {
    pub(crate) struct SecretKey {
        pub(crate) cursor_secret_key: {
            var_name: "GQL_API_CURSOR_SECRET_KEY",
            layers: [
                parsed<SecretKeyType>(parse_secret_key),
            ],
            description: "The secret key used for signing cursors for pagination \
                in the GraphQL API",
        }
    }
}

#[cfg(not(debug_assertions))]
mkenv::make_config! {
    pub(crate) struct SecretKey {
        pub(crate) cursor_secret_key: {
            var_name: "GQL_API_CURSOR_SECRET_KEY_FILE",
            layers: [
                file_read(),
                parsed<SecretKeyType>(parse_secret_key),
            ],
            description: "The path to the file containing The secret key used for signing cursors \
                for pagination in the GraphQL API",
        }
    }
}

mkenv::make_config! {
    pub(crate) struct ApiConfig {
        pub(crate) cursor_max_limit: {
            var_name: "GQL_API_CURSOR_MAX_LIMIT",
            layers: [
                parsed_from_str<usize>(),
                or_default_val(|| 100),
            ],
        },

        pub(crate) cursor_default_limit: {
            var_name: "GQL_API_CURSOR_DEFAULT_LIMIT",
            layers: [
                parsed_from_str<usize>(),
                or_default_val(|| 50),
            ],
        },

        pub(crate) cursor_secret_key: { SecretKey },
    }
}

static CONFIG: OnceLock<ApiConfig> = OnceLock::new();

#[derive(Debug)]
pub enum InitError {
    ConfigAlreadySet,
    Config(Box<dyn Error>),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::ConfigAlreadySet => f.write_str("config already set"),
            InitError::Config(error) => write!(f, "error during config init: {error}"),
        }
    }
}

impl Error for InitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            InitError::ConfigAlreadySet => None,
            InitError::Config(error) => Some(&**error),
        }
    }
}

pub fn init_config() -> Result<(), InitError> {
    let config = ApiConfig::define();
    config
        .try_init()
        .map_err(|e| InitError::Config(format!("{e}").into()))?;
    CONFIG.set(config).map_err(|_| InitError::ConfigAlreadySet)
}

pub(crate) fn config() -> &'static ApiConfig {
    CONFIG.get().unwrap()
}
