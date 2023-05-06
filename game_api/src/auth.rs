//! The authentication system.
//!
//! The authentication system is used to authenticate the client (which corresponds to
//! the Obstacle TitlePack Script). The authentication system is used to prevent
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

use std::convert::Infallible;
use std::{collections::HashMap, sync::Arc, time::Duration};

use actix_web::dev::ServiceRequest;
use actix_web::error::ParseError;
use actix_web::guard::{Guard, GuardContext};
use actix_web::http::header::{Header, TryIntoHeaderValue};
use actix_web::web::Data;
use actix_web::{Error, HttpMessage};
use chrono::{DateTime, Utc};
use deadpool_redis::redis::{cmd, AsyncCommands};
use rand::Rng;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot::{self, Receiver, Sender};
use tokio::time::timeout;
use tokio::{
    sync::{Mutex, RwLock},
    time::Instant,
};
use tracing::Level;

use crate::models::Role;
use crate::{Database, RecordsError, RecordsResult};

/// The client's token expires in 12 hours.
// const EXPIRES_IN: Duration = Duration::from_secs(60 * 60 * 12);
const EXPIRES_IN: Duration = Duration::from_secs(15);

/// The state expires in 5 minutes.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 5);

/// The server auth state is updated every day.
///
/// This is used to remove expired tokens.
pub const UPDATE_RATE: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug)]
struct InputsState {
    tx: Sender<String>,
    instant: DateTime<Utc>,
}

impl From<Sender<String>> for InputsState {
    fn from(tx: Sender<String>) -> Self {
        Self {
            tx,
            instant: Utc::now(),
        }
    }
}

#[derive(Debug)]
struct TokenState {
    tx: Sender<String>,
    rx: Receiver<String>,
    instant: DateTime<Utc>,
}

impl TokenState {
    fn new(tx: Sender<String>, rx: Receiver<String>) -> Self {
        Self {
            tx,
            rx,
            instant: Utc::now(),
        }
    }
}

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

/// Holds the state of the authentication system.
///
/// The state is used to prevent multiple requests to the /new_token endpoint
/// with the same state. This is to prevent CSRF attacks.
///
/// The state is also used to keep track of the client tokens. This is to
/// prevent the same client from logging in multiple times.
#[derive(Debug, Default)]
pub struct AuthState {
    inputs_states_map: Arc<Mutex<HashMap<String, InputsState>>>,
    token_states_map: Arc<Mutex<HashMap<String, TokenState>>>,
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

        tracing::event!(
            Level::INFO,
            "Removed {} expired token(s) from the client_map, {} token(s) in total",
            expired.len(),
            client_map.len()
        );
    }

    /// Removes a state from the state map.
    pub async fn remove_state(&self, state: String) {
        let mut state_map = self.inputs_states_map.lock().await;
        state_map.remove(&state);
        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the state_map, {} state(s) in total",
            state,
            state_map.len()
        );
    }

    pub async fn get_inputs_path(&self, state: String) -> RecordsResult<String> {
        let (tx, rx) = oneshot::channel();

        {
            let mut state_map = self.inputs_states_map.lock().await;
            let state_info = state.clone();
            if let Some(InputsState { instant, .. }) = state_map.get(&state) {
                return Err(RecordsError::StateAlreadyReceived(*instant));
            }
            state_map.insert(state.clone(), InputsState::from(tx));
            tracing::event!(
                Level::INFO,
                "Inserted state `{}` into the inputs_state_map, {} state(s) in total",
                state_info,
                state_map.len()
            );
        }

        match timeout(TIMEOUT, rx).await {
            Ok(Ok(inputs_path)) => Ok(inputs_path),
            _ => {
                tracing::event!(
                    Level::WARN,
                    "Inputs state `{}` timed out, removing it",
                    state
                );
                self.remove_state(state).await;
                Err(RecordsError::Timeout)
            }
        }
    }

    pub async fn connect_with_browser(
        &self,
        state: String,
    ) -> RecordsResult<(Sender<String>, Receiver<String>)> {
        // cross channels
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();

        // store channel in the state_map
        {
            let mut state_map = self.token_states_map.lock().await;
            let state_info = state.clone();
            if let Some(TokenState { instant, .. }) = state_map.get(&state) {
                return Err(RecordsError::StateAlreadyReceived(*instant));
            }
            state_map.insert(state.clone(), TokenState::new(tx2, rx1));
            tracing::event!(
                Level::INFO,
                "Inserted state `{}` into the token_state_map, {} state(s) in total",
                state_info,
                state_map.len()
            );
        }

        Ok((tx1, rx2))
    }

    /// Sends the sender of the /new_player_finished request
    /// to the /gen_new_token request's oneshot receiver.
    ///
    /// To synchronize the two requests, the state has to be the same. If the provided state
    /// isn't present in the state_map, it will return an error.
    ///
    /// Thus, this method should be called from the /gen_new_token request, and so this request
    /// should be called after the /new_player_finished request has been called.
    pub async fn inputs_received_for(
        &self,
        state: String,
        inputs_path: String,
    ) -> RecordsResult<()> {
        let mut state_map = self.inputs_states_map.lock().await;

        if let Some(InputsState { tx, .. }) = state_map.remove(&state) {
            tx.send(inputs_path)
                .expect("/player_finished rx should not be dropped at this point");
        } else {
            return Err(RecordsError::MissingPlayerFinishedReq(state));
        }

        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the inputs_state_map, {} state(s) in total",
            state,
            state_map.len()
        );
        Ok(())
    }

    pub async fn browser_connected_for(
        &self,
        state: String,
        access_token: String,
    ) -> RecordsResult<()> {
        let mut state_map = self.token_states_map.lock().await;

        if let Some(TokenState { tx, rx, .. }) = state_map.remove(&state) {
            tx.send(access_token)
                .expect("/player/get_token rx should not be dropped at this point");

            match timeout(TIMEOUT, rx).await {
                Ok(Ok(res)) => match res.as_str() {
                    "OK" => {}
                    "INVALID_TOKEN" => return Err(RecordsError::InvalidMPToken),
                    _ => unreachable!(),
                },
                _ => {
                    tracing::event!(Level::WARN, "Token state `{}` timed out", state);
                    return Err(RecordsError::Timeout);
                }
            };
        } else {
            return Err(RecordsError::MissingGetTokenReq);
        }

        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the token_state_map, {} state(s) in total",
            state,
            state_map.len()
        );

        Ok(())
    }

    /// Checks if the given token exists, corresponds to the given player login, and is still available.
    ///
    /// If the given token exists and corresponds to the given player login, but the TTL is expired,
    /// the token is removed from the client_map.
    pub async fn check_auth_for<B: ExtractAuthFields>(
        &self,
        db: &Database,
        min_req_role: Role,
        body: &B,
    ) -> RecordsResult<()> {
        let AuthFields { token, login } = body.get_auth_fields();
        let remove_client = {
            let client_map = self.client_map.read().await;
            match client_map.get(token) {
                Some(client) if &client.login == login => {
                    let client_role = sqlx::query_as::<_, Role>(
                        "SELECT * FROM role
                    WHERE id = (SELECT role FROM players WHERE login = ?)",
                    )
                    .bind(&client.login)
                    .fetch_one(&db.mysql_pool)
                    .await?;

                    if client.instant.elapsed() >= EXPIRES_IN || min_req_role > client_role {
                        true
                    } else {
                        return Ok(());
                    }
                }
                _ => false,
            }
        };

        if remove_client {
            let mut client_map = self.client_map.write().await;
            client_map.remove(token);
            tracing::event!(
                Level::INFO,
                "Removed token `{}` from the client_map, {} token(s) in total",
                token,
                client_map.len()
            );
        }

        Err(RecordsError::Unauthorized)
    }
}

pub async fn gen_token_for(db: &Database, login: String) -> RecordsResult<String> {
    let token = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(256)
        .collect::<Vec<u8>>();
    let token = String::from_utf8(token).expect("random token not utf8");
    let mut connection = db.redis_pool.get().await?;
    let key = format!("token:{login}");

    cmd("SET")
        .arg(&key)
        .arg(&token)
        .arg("EX")
        .arg(EXPIRES_IN.as_secs())
        .query_async(&mut connection)
        .await?;
    Ok(token)
}

async fn get_permissions_for(db: &Database, login: &str, token: &str) -> RecordsResult<Vec<Role>> {
    let mut connection = db.redis_pool.get().await?;
    let key = format!("token:{login}");
    let stored_token: Option<String> = connection.get(&key).await?;
    match stored_token {
        Some(t) if t == token => (),
        _ => return Ok(Vec::new()),
    }

    let roles = sqlx::query_as(
        "SELECT * FROM role WHERE id <= (SELECT role FROM players WHERE login = ?)
            ORDER BY id ASC",
    )
    .bind(&login)
    .fetch_all(&db.mysql_pool)
    .await?;

    Ok(roles)
}

#[derive(Serialize, Deserialize)]
pub struct AuthFields<'a> {
    pub token: &'a str,
    pub login: &'a str,
}

pub trait ExtractAuthFields {
    fn get_auth_fields(&self) -> AuthFields;
}

pub async fn auth_extractor(req: &ServiceRequest) -> Result<Vec<Role>, Error> {
    fn extract_header<'a>(req: &'a ServiceRequest, header: &str) -> Result<Option<&'a str>, Error> {
        req.headers()
            .get(header)
            .map(|value| value.to_str())
            .transpose()
            .map_err(|_| Error::from(RecordsError::Unauthorized))
    }

    let db = req
        .app_data::<Data<Database>>()
        .expect("Database not set as app data");

    let Some(login) = extract_header(req, "PlayerLogin")? else {
        return Ok(Vec::new());
    };
    let Some(token) = extract_header(req, "Authorization")? else {
        return Ok(Vec::new());
    };

    Ok(get_permissions_for(&db, login, token).await?)
}
