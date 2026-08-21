//! The main crate of the ShootMania Obstacle API infrastructure.
//!
//! The project is separated into several crates, including, in particular:
//! - `records_lib` (this crate): used by all the services related to the API. Mostly contains
//!   utility functions related to the entities saved in the database;
//! - [`graphql-api`](../graphql_api/index.html): the GraphQL API, including the various types
//!   referenced in the schema, and functions to generate the latter;
//! - [`game_api`](../game_api_lib/index.html): the in-game API itself, including the server with
//!   the endpoints, and everything that goes with it. It also hosts the `/graphql` endpoint,
//!   based on the schema provided by `graphql-api`.

#![warn(missing_docs)]
#![cfg_attr(nightly, feature(doc_cfg))]

mod env;
mod expirable;
mod mptypes;

pub mod error;
pub mod event;
pub mod leaderboard;
pub mod map;
pub mod mappack;
pub mod must;
pub mod opt_event;
pub mod player;
pub mod pool;
pub mod ranks;
pub mod records_notifier;
pub mod redis_key;
pub mod sync;
pub mod time;

/// The MySQL/MariaDB pool type.
pub type MySqlPool = sqlx::MySqlPool;
/// The Redis pool type.
pub type RedisPool = deadpool_redis::Pool;
/// A mutable reference to the connection to the database.
pub type MySqlConnection<'a> = &'a mut sqlx::pool::PoolConnection<sqlx::MySql>;
/// The type of a Redis connection.
pub type RedisConnection = deadpool_redis::Connection;

use std::future::Future;

pub use env::*;
pub use expirable::Expirable;
pub use mptypes::*;
pub use pool::Database;
use rand::Rng as _;

/// Asserts that the type of the provided future is Send, and returns an opaque type from it.
///
/// This helps the compiler to correctly type the values of some await points, and helps
/// to trace the root of weird errors.
#[inline(always)]
pub fn assert_future_send<T, R>(t: T) -> impl Future<Output = R> + Send
where
    T: Future<Output = R> + Send,
{
    t
}

/// Returns a randomly-generated string with the `len` length. It contains alphanumeric characters.
pub fn gen_random_str(len: usize) -> String {
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .map(char::from)
        .take(len)
        .collect()
}
