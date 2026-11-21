//! The HTTP client every request to MX goes through.
//!
//! This module actually only contains utility functions.

use futures::{StreamExt as _, TryStreamExt as _, stream};
use records_lib::error::RecordsResult;
use reqwest::header;
use serde::de::DeserializeOwned;

/// The amount of requests sent concurrently to MX for a single batch.
pub(crate) const MAX_CONCURRENT_REQUESTS: usize = 10;

const MX_USER_AGENT: &str = concat!(
    "ShootMania Obstacle API v",
    env!("API_VERSION"),
    " (Discord: @ahmadbky)"
);

/// The HTTP client talking to the MX API.
///
/// It implements [`Fetcher`](crate::polite::Fetcher) once per endpoint we call, which is what puts
/// every one of them behind the same politeness rules.
#[derive(Clone)]
pub struct MxApi(reqwest::Client);

impl MxApi {
    /// Wraps the provided HTTP client, which is shared with the rest of the application.
    #[inline]
    pub fn new(client: reqwest::Client) -> Self {
        Self(client)
    }

    /// Starts a `GET` on the MX API.
    pub(crate) fn get(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.0.get(url).header(header::USER_AGENT, MX_USER_AGENT)
    }
}

/// Runs the provided requests, at most [`MAX_CONCURRENT_REQUESTS`] at a time, and gathers what
/// they answered.
///
/// One failure fails the lot: a batch is written down as a whole or not at all, so a half-answer
/// would be remembered as "MX doesn't have the rest".
pub(crate) async fn gather<T, F>(requests: Vec<F>) -> RecordsResult<Vec<T>>
where
    F: Future<Output = RecordsResult<T>>,
{
    stream::iter(requests)
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .try_collect()
        .await
}

/// Sends a request and reads its JSON body.
pub(crate) async fn json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> RecordsResult<T> {
    req.send()
        .await?
        .error_for_status()?
        .json()
        .await
        .map_err(From::from)
}

/// Sends a request about one thing, and reads its JSON body.
///
/// A `404` is [`None`] rather than a failure: it's MX answering that it doesn't have this one,
/// which the caller remembers like any other answer. On the endpoints which take several keys at
/// once, MX says the same thing by leaving them out of its answer.
pub(crate) async fn json_or_absent<T: DeserializeOwned>(
    req: reqwest::RequestBuilder,
) -> RecordsResult<Option<T>> {
    let response = req.send().await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    response
        .error_for_status()?
        .json()
        .await
        .map(Some)
        .map_err(From::from)
}
