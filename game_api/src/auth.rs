//! The authentication system.
//!
//! The authentication system is used to authenticate the client (which corresponds to
//! the Obstacle Titlepack Script). The authentication system is used to prevent
//! unauthorized access to the API.
//!
//! The authentication system is based on the OAuth 2.0 protocol. It is only used when
//! a player finishes a map. When a new player finishes a map, the script must send a
//! /new_player_finished request to this server, and ask the player to log in to ManiaPlanet.
//! Then it will receive a token. This token is then sent to this server, to verify the token,
//! and to validate the map finish. After this procedure, the server generates a separate token
//! for the new player, and replies with it to the client. So the client can store the token
//! in the player's local files, and use it to authenticate the player when he finishes a map.
//!
//! The generated token has a time-to-live of 1 year. After this time, the player will have
//! to log in again to ManiaPlanet to get a new token.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use rand::Rng;
use records_lib::RecordsError;
use tokio::{
    sync::{mpsc::Sender, oneshot, Mutex, RwLock},
    time::Instant,
};
use tracing::Level;

/// The client's token expires in 1 year.
const EXPIRES_IN: Duration = Duration::from_secs(60 * 60 * 24 * 30 * 365);

/// The state expires in 5 minutes.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 5);

/// The server auth state is updated every day.
///
/// This is used to remove expired tokens.
pub const UPDATE_RATE: Duration = Duration::from_secs(60 * 60 * 24);

/// A client is a user that has logged in.
#[derive(Debug)]
struct Client {
    // The login of the player.
    login: String,
    // The date the token entry was created.
    instant: Instant,
}

impl From<String> for Client {
    fn from(login: String) -> Self {
        Self {
            login,
            instant: Instant::now(),
        }
    }
}

/// The sender of the channel which connects the /new_player_finished endpoint
/// to the /gen_new_token endpoint.
pub type SSender = Sender<String>;

/// The sender of the channel which synchronizes the /new_player_finished endpoint
/// with the /gen_new_token endpoint.
pub type OSSender = oneshot::Sender<SSender>;

/// A synchronization state of the authentication system.
///
/// It stores an instant, so it can be used to clear the state map, in case
/// a client does not send a request to the /gen_new_token endpoint, or has crashed before.
#[derive(Debug)]
struct State {
    os_tx: OSSender,
    tx: SSender,
    instant: DateTime<Utc>,
}

impl State {
    fn new(os_tx: OSSender, tx: SSender) -> Self {
        Self {
            os_tx,
            tx,
            instant: Utc::now(),
        }
    }

    fn is_expired(&self) -> bool {
        Utc::now().time() - self.instant.time() > chrono::Duration::from_std(TIMEOUT).unwrap()
    }

    fn into_channels(self) -> (OSSender, SSender) {
        (self.os_tx, self.tx)
    }
}

/// Holds the state of the authentication system.
///
/// The state is used to prevent multiple requests to the /new_token endpoint
/// with the same state. This is to prevent CSRF attacks.
///
/// The state is also used to keep track of the client tokens. This is to
/// prevent the same client from logging in multiple times.
#[derive(Debug, Default)]
pub struct AuthState {
    state_map: Arc<Mutex<HashMap<String, State>>>,
    // Client Obstacle token as key
    client_map: Arc<RwLock<HashMap<String, Client>>>,
}

impl AuthState {
    /// Updates the state of the authentication system.
    ///
    /// This is used to remove expired tokens and states.
    pub async fn update(&self) {
        let mut client_map = self.client_map.write().await;
        let expired = client_map
            .iter()
            .filter(|(_, client)| client.instant.elapsed() > EXPIRES_IN)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in &expired {
            client_map.remove(token);
        }

        tracing::event! {
            Level::INFO,
            "Removed {} expired token(s) from the client_map, {} token(s) in total",
            expired.len(),
            client_map.len()
        }

        drop(client_map);

        let mut state_map = self.state_map.lock().await;
        let expired = state_map
            .iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(state, _)| state.clone())
            .collect::<Vec<_>>();
        for state in &expired {
            state_map.remove(state);
        }

        tracing::event! {
            Level::INFO,
            "Removed {} expired state(s) from the state_map, {} state(s) in total",
            expired.len(),
            state_map.len()
        }
    }

    /// Removes a state from the state map.
    pub async fn remove_state(&self, state: String) {
        let mut state_map = self.state_map.lock().await;
        let state_info = state.to_string();
        state_map.remove(&state);
        tracing::event! {
            Level::INFO,
            "Removed state `{}` from the state_map, {} state(s) in total",
            state_info,
            state_map.len()
        }
    }

    /// Inserts a state into the state map.
    ///
    /// If the state already exists, it returns an error.
    ///
    /// This method should be called from the /new_player_finished request.
    pub async fn insert_state(
        &self, state: String, os_tx: oneshot::Sender<SSender>, tx: SSender,
    ) -> Result<(), RecordsError> {
        let mut state_map = self.state_map.lock().await;
        let state_info = state.clone();
        if let Some(State { instant, .. }) = state_map.get(&state) {
            return Err(RecordsError::StateAlreadyReceived(instant.clone()));
        }
        state_map.insert(state, State::new(os_tx, tx));
        tracing::event! {
            Level::INFO,
            "Inserted state `{}` into the state_map, {} state(s) in total",
            state_info,
            state_map.len()
        };
        Ok(())
    }

    /// Sends the sender of the /new_player_finished request
    /// to the /gen_new_token request's oneshot receiver.
    ///
    /// To synchronize the two requests, the state has to be the same. If the provided state
    /// isn't present in the state_map, it will return an error.
    ///
    /// Thus, this method should be called from the /gen_new_token request, and so this request
    /// should be called after the /new_player_finished request has been called.
    pub async fn exchange_tx(
        &self, state: String, gen_token_os_tx: oneshot::Sender<SSender>, tx: SSender,
    ) -> Result<(), RecordsError> {
        let mut state_map = self.state_map.lock().await;
        let (np_os_tx, np_tx) = match state_map.remove(&state).map(State::into_channels) {
            Some((os_tx, np_tx)) => (os_tx, np_tx),
            _ => return Err(RecordsError::MissingNewPlayerFinishReq),
        };
        np_os_tx.send(tx).unwrap();
        gen_token_os_tx.send(np_tx).unwrap();
        tracing::event! {
            Level::INFO,
            "Removed state `{}` from the state_map, {} state(s) in total",
            state,
            state_map.len()
        }
        Ok(())
    }

    /// Generates a new token for the given player described by his ManiaPlanet login.
    ///
    /// The new token is inserted in the client_map.
    pub async fn gen_token_for(&self, login: String) -> String {
        let mut client_map = self.client_map.write().await;
        let mut token = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(30)
            .collect::<Vec<u8>>();
        token.extend(Utc::now().timestamp().to_be_bytes());
        token.extend(login.as_bytes());
        let token = String::from_utf8(token).unwrap();
        client_map.insert(token.clone(), Client::from(login.clone()));
        tracing::event! {
            Level::INFO,
            "Inserted token `{}` for `{}` into the client_map, {} token(s) in total",
            token, login, client_map.len()
        };
        token
    }

    /// Checks if the given token exists, corresponds to the given player login, and is still available.
    /// 
    /// If the given token exists and corresponds to the given player login, but the TTL is expired,
    /// the token is removed from the clien_map.
    pub async fn check_token_for(&self, token: &str, login: &str) -> bool {
        let client_map = self.client_map.read().await;
        let remove_client = match client_map.get(token) {
            Some(client) if client.login == login => {
                if client.instant.elapsed() >= EXPIRES_IN {
                    true
                } else {
                    return true;
                }
            }
            _ => false,
        };

        if remove_client {
            let mut client_map = self.client_map.write().await;
            client_map.remove(token);
            tracing::event! {
                Level::INFO,
                "Removed token `{}` from the client_map, {} token(s) in total",
                token, client_map.len()
            };
        }

        false
    }
}
