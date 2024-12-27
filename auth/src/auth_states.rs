use core::fmt;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Once},
};

use once_cell::sync::OnceCell;
use secure_string::SecureString;
use tokio::sync::{oneshot, Mutex, Notify, RwLock};

use crate::{browser::CodeValidationErr, game::InvalidAccessToken, generated, Code, StateId};

#[derive(Debug)]
pub(crate) enum AuthState {
    /// The current state is transitioning.
    Transition,
    /// The game endpoint requested an authentication, and we're waiting for the beginning
    /// of the process.
    WaitingForAuthentication,
    /// The first step of the authentication has been triggered by the game endpoint.
    OAuthByGame(
        /// The sender of the ManiaPlanet access token.
        oneshot::Sender<SecureString>,
        /// The receiver of the game endpoint about the validity of this access token.
        oneshot::Receiver<Result<(), InvalidAccessToken>>,
    ),
    /// The first step of the authentication has been triggered by the browser endpoint.
    OAuthByBrowser(
        /// The sender of the validity of the access token.
        oneshot::Sender<Result<(), InvalidAccessToken>>,
        /// The receiver of the access token.
        oneshot::Receiver<SecureString>,
    ),
    /// The state that comes right after the OAuth authentication step, and
    /// corresponds to the generation of a code meant to be displayed on the player's browser.
    CodeGenerated {
        /// The hash of the generated code
        hash: generated::CodeHash,
    },
    /// The second step of the authentication has been triggered by the game endpoint, and
    /// the entered code was valid.
    CodeValidByGame,
    /// The second step of the authentication has been triggered by the game endpoint, and
    /// the entered code was invalid.
    CodeInvalidByGame {
        /// The hash of the correct generated code.
        hash: generated::CodeHash,
        /// The amount of tries that was attempted.
        tries: NonZeroUsize,
    },
    /// The second step of the authentication has been triggered by the game endpoint, and
    /// the entered code was invalid and attempted too many times, so the authentication failed.
    CodeInvalidTerminate,
    /// The second step of the authentication has been triggered by the browser endpoint.
    CodeByBrowser(
        /// The sender of the entered code, by the game endpoint.
        oneshot::Sender<Code>,
        /// The receiver of the code validation result, by the browser endpoint.
        oneshot::Receiver<Result<(), CodeValidationErr>>,
    ),
}

impl fmt::Display for AuthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transition => f.write_str("Transition state"),
            Self::WaitingForAuthentication => f.write_str("Waiting for authentication"),
            Self::OAuthByGame(..) | Self::OAuthByBrowser(..) => f.write_str("OAuth state"),
            Self::CodeGenerated { .. }
            | Self::CodeValidByGame
            | Self::CodeInvalidByGame { .. }
            | Self::CodeInvalidTerminate
            | Self::CodeByBrowser(..) => f.write_str("Code state"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct AuthEntry {
    state: AuthState,
    at: chrono::DateTime<chrono::Utc>,
    timeout_notify: Arc<Notify>,
}

impl fmt::Display for AuthEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "since {at}: {state}", at = self.at, state = self.state)
    }
}

impl AuthEntry {
    /// Creates a new authentication entry with the [`AuthState::WaitingForAuthentication`] state.
    pub(crate) fn new(timeout_notify: Arc<Notify>) -> Self {
        Self {
            state: AuthState::WaitingForAuthentication,
            at: chrono::Utc::now(),
            timeout_notify,
        }
    }

    /// Updates the authentication state of this entry.
    pub(crate) fn set_state(&mut self, new: AuthState) {
        self.at = chrono::Utc::now();
        self.timeout_notify.notify_one();
        self.state = new;
    }

    /// Returns the old authentication state of this entry, leaving it as transitioning.
    #[inline(always)]
    pub(crate) fn transition(&mut self) -> AuthState {
        std::mem::replace(&mut self.state, AuthState::Transition)
    }

    /// Crosses 2 channels of different types.
    ///
    /// ```text
    ///  game endpoint        browser endpoint
    ///                                   
    ///      Sender<A> -\  /- Sender<B>       
    ///                  \/                 
    ///                  /\                 
    ///    Receiver<B> -/  \- Receiver<A>     
    /// ```
    /// 
    /// This way, the game endpoint can send some data of type `A` to the browser endpoint,
    /// and the latter can respond with some data of type `B`.
    pub(crate) fn cross_channel<F, A, B>(
        &mut self,
        with: F,
    ) -> (oneshot::Sender<A>, oneshot::Receiver<B>)
    where
        F: FnOnce(oneshot::Sender<B>, oneshot::Receiver<A>) -> AuthState,
    {
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        self.set_state(with(tx1, rx2));
        (tx2, rx1)
    }
}

type AuthStatesMap = RwLock<HashMap<StateId, Mutex<AuthEntry>>>;
static AUTH_STATES: OnceCell<AuthStatesMap> = OnceCell::new();

/// Initializes the authentication system.
///
/// This function **must** be called once, at the start of the program,
/// before performing any action with the authentication system.
/// This goes for the tests too.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        AUTH_STATES
            .set(Default::default())
            .expect("auth states cell was already initialized");
    });
}

pub(crate) fn auth_states() -> &'static AuthStatesMap {
    debug_assert!(
        AUTH_STATES.get().is_some(),
        "auth states map isn't initialized"
    );
    // SAFETY: the program should call the above init() function
    unsafe { AUTH_STATES.get_unchecked() }
}

#[derive(thiserror::Error, Debug)]
#[error("state not received")]
pub struct StateNotReceived;

/// An error occurred when dealing with authentication states.
#[derive(thiserror::Error, Debug)]
pub enum StateError {
    /// The authentication state hasn't been received (yet).
    #[error(transparent)]
    StateNotReceived(#[from] StateNotReceived),
    /// The authentication state was unexpected.
    #[error("invalid auth state")]
    InvalidAuthState,
}

/// Called by each endpoint to start a synchronization process.
/// 
/// This function allows to get a mutable handle to the authentication entry.
pub(crate) async fn sync<F, T, E>(state_id: StateId, f: F) -> Result<T, E>
where
    F: FnOnce(&mut AuthEntry) -> Result<T, E>,
    E: From<StateNotReceived>,
{
    let auth_states = auth_states().read().await;
    let Some(auth_entry) = auth_states.get(&state_id) else {
        return Err(From::from(StateNotReceived));
    };
    let mut auth_entry = auth_entry.lock().await;
    f(&mut auth_entry)
}
