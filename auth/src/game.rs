//! Module that contains the functions available for the game endpoint.
//!
//! These functions communicate with the browser endpoint by a unique authentication state ID, so
//! they should be called in a precise order.
//!
//! More precisely, the game endpoint should first call the [`request_auth`] function,
//! then [`wait_for_oauth`] with the returned authentication state ID. Finally, after the
//! browser endpoint returned the numeric code to the player on their browser, the game endpoint should call
//! the [`validate_code`] function.
//!
//! See [the library documentation](super) for more information about the Obstacle authentication system.

use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::Arc;

use secure_string::SecureString;
use tokio::sync::{oneshot, Mutex, Notify};

use crate::auth_states::{auth_states, sync, AuthEntry, StateNotReceived};
use crate::browser::CodeValidationErr;
use crate::generated::CodeHash;
use crate::{
    AuthState, Code, CommonEndpointErr, CommonErr, RecvOrOut, SendOrClosed, StateError, StateId,
    Timeout as _, Token, CODE_MAX_TRIES,
};

/// The error type when there are too many authentication requests.
#[derive(thiserror::Error, Debug)]
#[error("too many authentication requests")]
pub struct TooManyRequests;

/// Requests an authentication, by getting an authentication state ID.
/// The returned string corresponds to the ID of the player's authentication along the whole process.
///
/// # Return
///
/// This function returns a random string ID, used to identify the authentication state of the player
/// during the process for both the game and browser endpoints.
///
/// It might return an error if there are too many authentication requests at the moment.
/// This avoids potential overload attacks.
pub async fn request_auth() -> Result<StateId, TooManyRequests> {
    const MAX_REQUESTS: usize = 50;

    {
        // Return early if they're too many 🥺
        let auth_states = auth_states().read().await;
        if auth_states.len() > MAX_REQUESTS {
            return Err(TooManyRequests);
        }
    }

    let state_id = StateId::new();
    let timeout_notify = Arc::new(Notify::new());

    {
        let mut auth_states = auth_states().write().await;
        auth_states.insert(state_id, Mutex::new(AuthEntry::new(timeout_notify.clone())));
    }

    tokio::task::spawn(async move {
        // Loop until we timeout
        while timeout_notify.notified().timeout().await.is_ok() {}

        let mut auth_states = auth_states().write().await;
        #[cfg(feature = "tracing")]
        tracing::info!("Removing authentication state `{state_id}`");
        auth_states.remove(&state_id);
    });

    Ok(state_id)
}

/// The error type that represents that the access token sent by the browser endpoint was invalid.
#[derive(thiserror::Error, Debug)]
#[error("invalid access token")]
pub struct InvalidAccessToken;

/// The error type used to represent the errors that could happen
/// when calling the [`wait_for_oauth`] function.
///
/// This struct is generic because the access token check is external, and might return
/// an external error, which we store here generically.
#[derive(thiserror::Error, Debug)]
pub enum OAuthErr<E> {
    /// The browser endpoint sent an invalid access token. This is usually because the player
    /// logged in with a different account than the one used in game.
    #[error(transparent)]
    InvalidAccessToken(InvalidAccessToken),
    /// The access token check returned an error.
    #[error("error when checking the access token: {0}")]
    CheckAccessToken(E),
    #[error(transparent)]
    /// Another common error happened.
    Other(#[from] CommonEndpointErr),
}

/// Waits for the browser endpoint to send the ManiaPlanet OAuth access token of the player.
///
/// # Arguments
///
/// * `state_id`: The ID returned previously by the API when requesting an authentication
///   (see [`request_auth`]).
///
/// # Return
///
/// If everything went OK, the game-mode should then ask the player to enter the code that
/// is displayed on their browser. After that, the game endpoint provides the entered code
/// with the [`validate_code`] function.
///
/// See the documentation of [`OAuthErr`] for the possible errors to happen.
pub async fn wait_for_oauth<F, Fut, E>(
    state_id: StateId,
    check_access_token: F,
) -> Result<(), OAuthErr<E>>
where
    F: FnOnce(SecureString) -> Fut,
    Fut: Future<Output = Result<bool, E>>,
{
    let (tx, rx) = sync(state_id, |auth_entry| match auth_entry.transition() {
        AuthState::WaitingForAuthentication => Ok(auth_entry.cross_channel(AuthState::OAuthByGame)),
        AuthState::OAuthByBrowser(tx, rx) => Ok((tx, rx)),
        _ => Err(StateError::InvalidAuthState),
    })
    .await
    .common()?;

    match check_access_token(rx.recv_or_out().await?).await {
        Ok(true) => (),
        Ok(false) => return Err(OAuthErr::InvalidAccessToken(InvalidAccessToken)),
        Err(e) => return Err(OAuthErr::CheckAccessToken(e)),
    }

    tx.send_or_closed(Ok(()))?;

    Ok(())
}

/// The error type used to represent the errors that could happen
/// when calling the [`validate_code`] function.
#[derive(thiserror::Error, Debug)]
pub enum CodeErr {
    /// The browser endpoint sent an error when validating the provided code.
    #[error(transparent)]
    CodeValidation(#[from] CodeValidationErr),
    /// Another common error happened.
    #[error(transparent)]
    Other(#[from] CommonEndpointErr),
}

impl From<StateNotReceived> for CodeErr {
    fn from(value: StateNotReceived) -> Self {
        Self::Other(CommonEndpointErr::StateErr(From::from(value)))
    }
}

enum GameCodeCameFirst {
    YesAndValid,
    No(
        oneshot::Sender<Code>,
        oneshot::Receiver<Result<(), CodeValidationErr>>,
    ),
}

fn try_validate(
    auth_entry: &mut AuthEntry,
    state_id: StateId,
    code: &Code,
    hash: CodeHash,
    tries: usize,
) -> Result<GameCodeCameFirst, CodeValidationErr> {
    if code.hash_with(state_id) == hash {
        auth_entry.set_state(AuthState::CodeValidByGame);
        Ok(GameCodeCameFirst::YesAndValid)
    } else {
        let tries = NonZeroUsize::new(tries + 1).unwrap();
        auth_entry.set_state(AuthState::CodeInvalidByGame { hash, tries });
        Err(CodeValidationErr::Invalid)
    }
}

/// Confirms the code returned by the browser endpoint, and entered by the user in game.
///
/// # Arguments
///
/// * `state_id`: The authentication state ID used during the player authentication process.
/// * `code`: The code entered by the player in game.
///
/// # Return
///
/// If everything went OK, this function will return the Obstacle authentication token of the player.
/// This will be stored after by the game-mode and will be used in the future to authenticate the player.
///
/// See the documentation of [`CodeErr`] to see what errors could happen.
pub async fn validate_code(state_id: StateId, code: Code) -> Result<Token, CodeErr> {
    let GameCodeCameFirst::No(tx, rx) =
        sync(state_id, |auth_entry| match auth_entry.transition() {
            AuthState::CodeGenerated { hash } => {
                try_validate(auth_entry, state_id, &code, hash, 0).map_err(From::from)
            }
            AuthState::CodeInvalidByGame { tries, .. } if tries.get() >= CODE_MAX_TRIES => {
                auth_entry.set_state(AuthState::CodeInvalidTerminate);
                Err(CodeValidationErr::Failed.into())
            }
            AuthState::CodeInvalidByGame { hash, tries } => {
                try_validate(auth_entry, state_id, &code, hash, tries.get()).map_err(From::from)
            }
            AuthState::CodeByBrowser(tx, rx) => Ok(GameCodeCameFirst::No(tx, rx)),
            _ => Err(CodeErr::Other(CommonEndpointErr::StateErr(
                StateError::InvalidAuthState,
            ))),
        })
        .await?
    else {
        return Ok(Token::new());
    };

    // Send the code and validate it
    tx.send_or_closed(code)?;
    rx.recv_or_out().await??;

    Ok(Token::new())
}
