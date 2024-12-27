//! # The ShootMania Obstacle authentication system.
//!
//! Nothing better than a diagram about how this works:
#![doc = "<style>.actor-bottom { display: none; }</style>"]
#![doc = simple_mermaid::mermaid!("../seq_diagram.mmd")]
#![deny(clippy::all)]
#![deny(missing_docs)]
#![warn(unreachable_pub)]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time;

use self::auth_states::{AuthState, StateError};

#[cfg(test)]
mod tests;

mod auth_states;
mod generated;

pub mod browser;
pub mod game;

pub use crate::{
    auth_states::init,
    generated::{Code, StateId, Token},
};

const TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const CODE_MAX_TRIES: usize = 3;

/// The error type used to represent common errors that could happen
/// at an endpoint.
#[derive(thiserror::Error, Debug)]
pub enum CommonEndpointErr {
    /// An error occurred when trying to deal with the authentication state.
    #[error(transparent)]
    StateErr(#[from] StateError),
    /// An error occurred when trying to send something to the opposite endpoint.
    #[error("channel closed unexpectedly")]
    ChannelClosed,
    /// The opposite endpoint took too much time to respond.
    #[error("opposite endpoint took too much time to respond")]
    Timeout(#[from] time::error::Elapsed),
}

trait Timeout: Sized {
    fn timeout(self) -> time::Timeout<Self>;
}

impl<F, O> Timeout for F
where
    F: core::future::Future<Output = O>,
{
    fn timeout(self) -> time::Timeout<Self> {
        time::timeout(TIMEOUT, self)
    }
}

trait CommonErr<T> {
    fn common(self) -> Result<T, CommonEndpointErr>;
}

impl<T, E> CommonErr<T> for Result<T, E>
where
    CommonEndpointErr: From<E>,
{
    #[inline(always)]
    fn common(self) -> Result<T, CommonEndpointErr> {
        self.map_err(From::from)
    }
}

trait SendOrClosed<T> {
    fn send_or_closed(self, _: T) -> Result<(), CommonEndpointErr>;
}

impl<T> SendOrClosed<T> for oneshot::Sender<T> {
    #[inline(always)]
    fn send_or_closed(self, obj: T) -> Result<(), CommonEndpointErr> {
        self.send(obj).map_err(|_| CommonEndpointErr::ChannelClosed)
    }
}

struct RecvOrOutFut<Fut> {
    fut: time::Timeout<Fut>,
}

impl<T, Fut> Future for RecvOrOutFut<Fut>
where
    Fut: Future<Output = Result<T, oneshot::error::RecvError>>,
{
    type Output = Result<T, CommonEndpointErr>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: it's a field of the struct 😑
        let fut = unsafe { self.map_unchecked_mut(|s| &mut s.fut) };
        fut.poll(cx).map(|res| {
            res.map_err(CommonEndpointErr::from)
                .and_then(|res| res.map_err(|_| CommonEndpointErr::ChannelClosed))
        })
    }
}

trait RecvOrOut<T>: Sized {
    fn recv_or_out(self) -> RecvOrOutFut<Self>;
}

impl<T> RecvOrOut<T> for oneshot::Receiver<T> {
    fn recv_or_out(self) -> RecvOrOutFut<Self> {
        RecvOrOutFut {
            fut: self.timeout(),
        }
    }
}
