//! The ShootMania Obstacle API library that the program uses.
//!
//! The content of this library is only made for the API program.

pub use deadpool_redis::Pool as RedisPool;
pub use sqlx::MySqlPool;

mod auth;
mod discord_webhook;
mod env;
mod error;
mod graphql;
mod http;
mod modeversion;
pub(crate) mod must;
mod utils;

#[cfg(test)]
mod tests;

pub use auth::AuthState;
pub use env::*;
pub use error::*;
pub use graphql::graphql_route;
pub use http::api_route;
pub use modeversion::*;
pub use utils::Res;
