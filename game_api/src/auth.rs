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

use std::future::{ready, Ready};
use std::{collections::HashMap, time::Duration};

use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest};
use chrono::{DateTime, Utc};
use deadpool_redis::redis::{cmd, AsyncCommands};
use rand::Rng;
use tokio::sync::oneshot::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::Level;

use crate::{http::player, models::Role, Database, RecordsError, RecordsResult};

/// The client's token expires in 12 hours.
const EXPIRES_IN: Duration = Duration::from_secs(60 * 60 * 12);
//const EXPIRES_IN: Duration = Duration::from_secs(15);

/// The state expires in 5 minutes.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 5);

/// The server auth state is updated every day.
///
/// This is used to remove expired tokens.
pub const UPDATE_RATE: Duration = Duration::from_secs(60 * 60 * 24);

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

/// Holds the state of the authentication system.
///
/// The state is used to prevent multiple requests to the /new_token endpoint
/// with the same state. This is to prevent CSRF attacks.
///
/// The state is also used to keep track of the client tokens. This is to
/// prevent the same client from logging in multiple times.
#[derive(Debug, Default)]
pub struct AuthState {
    token_states_map: Mutex<HashMap<String, TokenState>>,
}

impl AuthState {
    /// Removes a state from the state map.
    pub async fn remove_state(&self, state: String) {
        let mut state_map = self.token_states_map.lock().await;
        state_map.remove(&state);
        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the state_map, {} state(s) in total",
            state,
            state_map.len()
        );
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

pub async fn check_auth_for(
    db: &Database,
    AuthHeader { login, token }: AuthHeader,
    required: Role,
) -> RecordsResult<()> {
    let mut connection = db.redis_pool.get().await?;
    let key = format!("token:{login}");
    let stored_token: Option<String> = connection.get(&key).await?;
    match stored_token {
        Some(t) if t == token => (),
        _ => return Err(RecordsError::Unauthorized),
    }

    let player = player::get_player_from_login(&db, &login)
        .await?
        .expect("invalid player in redis server");

    if let Some(ban) = player::check_banned(&db, player.id).await? {
        return Err(RecordsError::BannedPlayer(ban));
    };

    let role: Role =
        sqlx::query_as("SELECT * FROM role WHERE id = (SELECT role FROM players WHERE id = ?)")
            .bind(player.id)
            .fetch_one(&db.mysql_pool)
            .await?;

    if role < required {
        return Err(RecordsError::Unauthorized);
    }

    Ok(())
}

pub struct AuthHeader {
    pub login: String,
    pub token: String,
}

impl FromRequest for AuthHeader {
    type Error = RecordsError;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let ext_header = |header| {
            req.headers()
                .get(header)
                .and_then(|h| h.to_str().map(str::to_string).ok())
        };

        #[cfg(feature = "release")]
        {
            let (Some(agent), Some(login), Some(token)) = (
                ext_header("User-Agent"), 
                ext_header("PlayerLogin"),
                ext_header("Authorization"))
            else {
                return ready(Err(RecordsError::Unauthorized));
            };

            if !agent.starts_with("ManiaPlanet/") {
                return ready(Err(RecordsError::Unauthorized));
            }

            ready(Ok(Self { login, token }))
        }

        #[cfg(not(feature = "release"))]
        {
            let (Some(login), Some(token)) = (
                ext_header("PlayerLogin"),
                ext_header("Authorization"))
            else {
                return ready(Err(RecordsError::Unauthorized));
            };

            ready(Ok(Self { login, token }))
        }
    }
}