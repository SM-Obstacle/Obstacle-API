//! The authentication system.
//!
//! It is mainly used by the Obstacle Titlepack Script, to authenticate the records of the players,
//! and their actions. It allows them to update their information, save a record, rate a map, etc.
//!
//! The authentication system is also used by the website to retrieve and update information.
//!
//! For each player, the system produces 2 tokens:
//! * The Maniaplanet token, used by the Obstacle gamemode to register the in-game information
//! * The website token, used by the website to retrieve sensitive information
//!
//! For every API route that requires authentication, it will retrieve the `Authorization` header
//! bound with the `"login"` provided in the request payload, to check for the player's authenticity.
//! This goes for the Obstacle gamemode, but for the website, it will be a session id cookie
//! sent by the browser.
//!
//! For each case, the player's role (Player, Mod, Admin) is also checked to correspond
//! to the required role.
//!
//! The procedure to initialize the tokens uses the OAuth 2.0 protocol provided by ManiaPlanet.
//! It has only access to the `basic` scope, meaning the name of the player, his login,
//! and his zone path. For now, this procedure is done from the Obstacle Titlepack. It is as follows:
//!
//! 1. The script generates a random string named `state`
//! 2. It sends a POST request to `/player/get_token`, with the corresponding payload
//! 3. It waits a bit, then opens a URL for the player to
//! https://prod.live.maniaplanet.com/login/oauth/authorize?response_type=token&client_id=de1ce3ba8e&client_secret=52877e3c1aa428eeb75a042c52caa01fb74a7526&redirect_uri=https://obstacle.titlepack.io/give_token&state=`state`&scope=basic
//! 4. The player logs in with his ManiaPlanet account, and is redirected to the page at
//! https://obstacle.titlepack.io/give_token URL.
//! 5. This page executes a JavaScript code that sends a POST request to `/player/give_token`
//! with the access token provided by the ManiaPlanet OAuth system and the same `state`.
//! 6. The authentication system validates the provided access token, and generates the 2 tokens
//! for the player.
//! 7. The POST `/player/give_token` request returns a `200 OK` response with a `Set-Cookie` header
//! containing the encoded session ID with the website token stored in it bound with the player's login
//! 8. The POST `/player/get_token` request returns the response with the generated ManiaPlanet token
//! that will be used to authenticate the player in the gamemode.
//!
//! The generated tokens have a time-to-live of 6 months. Passed this time, the authentication system
//! will return an `Unauthorized` error. The gamemode script will have to execute the procedure
//! from above.
//!
//! See https://github.com/maniaplanet/documentation/blob/master/13.web-services/01.oauth2/docs.md#implicit-flow
//! for more information.

use std::future::{ready, Ready};
use std::sync::OnceLock;
use std::{collections::HashMap, time::Duration};

use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest};
use chrono::{DateTime, Utc};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::sync::oneshot::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::Level;

use crate::utils::{format_mp_token_key, format_web_token_key, generate_token};
use crate::{http::player, models::Role, Database, RecordsError, RecordsResult};

static EXPIRES_IN: OnceLock<u32> = OnceLock::new();

pub fn get_tokens_ttl() -> u32 {
    *EXPIRES_IN.get_or_init(|| {
        std::env::var("RECORDS_API_TOKEN_TTL")
            .expect("RECORDS_API_TOKEN_TTL env var is not set")
            .parse()
            .expect("RECORDS_API_TOKEN_TTL should be u32")
    })
}

pub const WEB_TOKEN_SESS_KEY: &str = "__obs_web_token";

/// The state expires in 5 minutes.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 5);

/// The server auth state is updated every day.
///
/// This is used to remove expired tokens.
pub const UPDATE_RATE: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebToken {
    pub login: String,
    pub token: String,
}

#[derive(Debug)]
struct TokenState {
    tx: Sender<Message>,
    rx: Receiver<Message>,
    instant: DateTime<Utc>,
}

impl TokenState {
    fn new(tx: Sender<Message>, rx: Receiver<Message>) -> Self {
        Self {
            tx,
            rx,
            instant: Utc::now(),
        }
    }
}

#[derive(Debug)]
pub enum Message {
    MPAccessToken(String),
    InvalidMPToken,
    Ok(WebToken),
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
    ) -> RecordsResult<(Sender<Message>, Receiver<Message>)> {
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
    ) -> RecordsResult<WebToken> {
        let mut state_map = self.token_states_map.lock().await;

        let web_token = if let Some(TokenState { tx, rx, .. }) = state_map.remove(&state) {
            tx.send(Message::MPAccessToken(access_token))
                .expect("/player/get_token rx should not be dropped at this point");

            match timeout(TIMEOUT, rx).await {
                Ok(Ok(res)) => match res {
                    Message::Ok(web_token) => web_token,
                    Message::InvalidMPToken => return Err(RecordsError::InvalidMPToken),
                    _ => unreachable!(),
                },
                _ => {
                    tracing::event!(Level::WARN, "Token state `{}` timed out", state);
                    return Err(RecordsError::Timeout);
                }
            }
        } else {
            return Err(RecordsError::MissingGetTokenReq);
        };

        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the token_state_map, {} state(s) in total",
            state,
            state_map.len()
        );

        Ok(web_token)
    }
}

/// Generates a ManiaPlanet and Website token for the player with the provided login.
///
/// The player might not yet exist in the database.
/// It returns a couple of (ManiaPlanet token ; Website token).
///
/// The tokens are stored in the Redis database.
pub async fn gen_token_for(db: &Database, login: &str) -> RecordsResult<(String, String)> {
    let mp_token = generate_token(256);
    let web_token = generate_token(32);

    let mut connection = db.redis_pool.get().await?;
    let mp_key = format_mp_token_key(&login);
    let web_key = format_web_token_key(&login);

    let ex = get_tokens_ttl() as usize;

    let mp_token_hash = digest(&*mp_token);
    let web_token_hash = digest(&*web_token);

    connection.set_ex(mp_key, mp_token_hash, ex).await?;
    connection.set_ex(web_key, web_token_hash, ex).await?;
    Ok((mp_token, web_token))
}

async fn inner_check_auth_for(
    db: &Database,
    login: String,
    token: &str,
    required: Role,
    key: String,
) -> RecordsResult<()> {
    let mut connection = db.redis_pool.get().await?;
    let stored_token: Option<String> = connection.get(&key).await?;
    if !matches!(stored_token, Some(t) if t == digest(token)) {
        return Err(RecordsError::Unauthorized);
    }

    // At this point, if Redis has registered a token with the login, it means that
    // the player is not yet added to the Obstacle database but effectively has a ManiaPlanet account
    let Some(player) = player::get_player_from_login(db, &login).await? else {
        return Err(RecordsError::PlayerNotFound(login));
    };

    if let Some(ban) = player::check_banned(db, player.id).await? {
        return Err(RecordsError::BannedPlayer(ban));
    };

    let role: Role = sqlx::query_as(
        "SELECT r.id, r.role_name FROM players p
            INNER JOIN role r ON r.id = p.role
            WHERE p.id = ?",
    )
    .bind(player.id)
    .fetch_one(&db.mysql_pool)
    .await?;

    if role < required {
        return Err(RecordsError::Forbidden);
    }

    Ok(())
}

pub async fn website_check_auth_for(
    db: &Database,
    web_token: WebToken,
    required: Role,
) -> RecordsResult<()> {
    let key = format_web_token_key(&web_token.login);
    inner_check_auth_for(db, web_token.login, &web_token.token, required, key).await
}

/// Checks for a successful authentication for the player with its login and ManiaPlanet token.
///
/// * If the token is invalid, or the player is banned, it returns an `Unauthorized` error
/// * If it is valid, but the player doesn't exist in the database, it returns a `PlayerNotFound` error
/// * If the player hasn't the required role, it returns a `Forbidden` error
/// * Otherwise, it returns Ok(())
pub async fn check_auth_for(
    db: &Database,
    AuthHeader { login, token }: AuthHeader,
    required: Role,
) -> RecordsResult<()> {
    let key = format_mp_token_key(&login);
    inner_check_auth_for(db, login, &token, required, key).await
}

#[derive(Clone)]
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

        let (Some(login), Some(token)) = (
            ext_header("PlayerLogin"),
            ext_header("Authorization"))
        else {
            return ready(Err(RecordsError::Unauthorized));
        };

        ready(Ok(Self { login, token }))
    }
}
