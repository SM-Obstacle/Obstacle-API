//! Module that contains the functions available for the browser endpoint.
//!
//! These functions communicate with the game endpoint by a unique authentication ID, so they should
//! be called in a precise order.
//!
//! More precisely, after the game endpoint requested an authentication and waits for the browser endpoint,
//! the latter should call [`notify_ingame`]. Finally, the browser should show the returned numeric code
//! to the player, and call [`wait_code_validation`] to wait for the game endpoint code confirmation.
//!
//! See [the library documentation](super) for more information about the Obstacle authentication system.

use secure_string::SecureString;
use tokio::sync::oneshot;

use crate::auth_states::{auth_states, sync, StateNotReceived};
use crate::game::InvalidAccessToken;
use crate::generated::CodeHash;
use crate::{
    AuthState, Code, CommonEndpointErr, CommonErr, RecvOrOut as _, SendOrClosed as _, StateError,
    StateId, CODE_MAX_TRIES,
};

/// The error type that represents a code validation error from the browser endpoint.
#[derive(thiserror::Error, Debug)]
pub enum CodeValidationErr {
    /// The code sent by the game endpoint is invalid. It can still retry.
    #[error("invalid code")]
    Invalid,
    /// The game endpoint sent too many wrong codes, so the authentication failed.
    #[error("code validation failed")]
    Failed,
}

/// The error type used to represent the errors that could happen
/// when calling the [`notify_ingame`] function.
#[derive(thiserror::Error, Debug)]
pub enum OAuthErr {
    /// The access token returned by the ManiaPlanet OAuth2 system is invalid.
    /// This usually means that the player logged in with a different account than
    /// the one used in game.
    #[error(transparent)]
    InvalidAccessToken(#[from] InvalidAccessToken),
    /// Another common error occurred.
    #[error(transparent)]
    Other(#[from] CommonEndpointErr),
}

/// Puts back the code hash in the authentication entry of the provided state id.
async fn put_gen_code_state(state_id: StateId, hash: CodeHash) {
    let auth_states = auth_states().read().await;
    // SAFETY: the caller guaranteed that the state id is in the authentication map
    let auth_entry = auth_states.get(&state_id).unwrap();
    let mut auth_entry = auth_entry.lock().await;
    auth_entry.set_state(AuthState::CodeGenerated { hash });
}

/// Notifies the game endpoint that the browser got the access token from the ManiaPlanet OAuth system.
///
/// # Arguments
///
/// * `state_id`: the authentication state ID used along the player's authentication process.
/// * `data`: The access token of the player returned by the ManiaPlanet OAuth system.
///
/// # Return
///
/// If everything went OK, this function returns a numeric code to be displayed to the player in
/// their browser. The browser endpoint should then immediately call the [`wait_code_validation`] function
/// to wait for the game endpoint response. The player will then have to enter this code in game.
///
/// See the documentation of [`OAuthErr`] to see what errors could happen.
pub async fn notify_ingame(state_id: StateId, data: SecureString) -> Result<Code, OAuthErr> {
    let (tx, rx) = sync(state_id, |auth_entry| match auth_entry.transition() {
        AuthState::WaitingForAuthentication => {
            Ok(auth_entry.cross_channel(AuthState::OAuthByBrowser))
        }
        AuthState::OAuthByGame(tx, rx) => Ok((tx, rx)),
        _ => Err(StateError::InvalidAuthState),
    })
    .await
    .common()?;

    // Check if the access token is valid or not
    tx.send_or_closed(data)?;
    // This returns early if the access token is invalid
    rx.recv_or_out().await??;

    let code = Code::new();

    put_gen_code_state(state_id, code.hash_with(state_id)).await;

    Ok(code)
}

/// The error type used to represent the errors that could happen when calling the [`wait_code_validation`]
/// function.
#[derive(thiserror::Error, Debug)]
pub enum CodeErr {
    /// The game endpoint gave too many wrong codes.
    #[error("code validation failed")]
    Failed,
    /// Another common error occurred.
    #[error(transparent)]
    Other(#[from] CommonEndpointErr),
}

impl From<StateNotReceived> for CodeErr {
    fn from(value: StateNotReceived) -> Self {
        Self::Other(CommonEndpointErr::StateErr(From::from(value)))
    }
}

/// Utility enum used to get the synchronization state of the code authentication state.
enum BrowserCodeCameFirst {
    /// The browser endpoint came first to process the second step of the authentication,
    /// so it creates a crossed channel and retrieves the generated code hash.
    Yes(
        /// The sender of the validation of the code by the browser endpoint.
        oneshot::Sender<Result<(), CodeValidationErr>>,
        /// The sender of the code by the game endpoint.
        oneshot::Receiver<Code>,
        /// The hash of the code retrieved from the authentication entry.
        CodeHash,
    ),
    /// The game endpoint came first to process the second step of the authentication,
    /// and it validated itself the entered code, so the authentication process is successful.
    NoAndValid,
    /// The game endpoint came first to process the second step of the authentication,
    /// it tried to validate itself the entered code, but the validation failed.
    NoAndInvalid,
}

/// Waits for the code confirmation of the game endpoint.
///
/// The code corresponds to the numeric code returned by [`notify_ingame`].
///
/// # Arguments
///
/// * `state_id`: the authentication state ID of the player during the process.
///
/// # Return
///
/// If everything went `Ok(())`, the browser endpoint will then create a cookie session for the player.
///
/// See the documentation of [`CodeErr`] to see what errors could happen.
pub async fn wait_code_validation(state_id: StateId) -> Result<(), CodeErr> {
    let mut i = 1;
    loop {
        let (tx, rx, hash) = match sync(state_id, |auth_entry| match auth_entry.transition() {
            AuthState::CodeGenerated { hash } => {
                let (tx, rx) = auth_entry.cross_channel(AuthState::CodeByBrowser);
                Ok(BrowserCodeCameFirst::Yes(tx, rx, hash))
            }
            AuthState::CodeValidByGame => Ok(BrowserCodeCameFirst::NoAndValid),
            AuthState::CodeInvalidByGame { tries, .. } => {
                i += tries.get() - 1;
                Ok(BrowserCodeCameFirst::NoAndInvalid)
            }
            AuthState::CodeInvalidTerminate => Err(CodeErr::Failed),
            _ => Err(CodeErr::Other(CommonEndpointErr::StateErr(
                StateError::InvalidAuthState,
            ))),
        })
        .await?
        {
            BrowserCodeCameFirst::Yes(tx, rx, hash) => (tx, rx, hash),
            BrowserCodeCameFirst::NoAndValid => return Ok(()),
            BrowserCodeCameFirst::NoAndInvalid => continue,
        };

        let code = rx.recv_or_out().await?;
        if code.hash_with(state_id) == hash {
            tx.send_or_closed(Ok(()))?;
            return Ok(());
        } else if i < CODE_MAX_TRIES {
            tx.send_or_closed(Err(CodeValidationErr::Invalid))?;
            i += 1;
            put_gen_code_state(state_id, hash).await;
        } else {
            tx.send_or_closed(Err(CodeValidationErr::Failed))?;
            break;
        }
    }

    Err(CodeErr::Failed)
}
