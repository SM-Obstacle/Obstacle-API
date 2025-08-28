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
//! For every API route that requires authentication, it will retrieve the `Authorization`
//! and `PlayerLogin` headers provided with the request, to check for the player's authenticity.
//! This goes for the Obstacle gamemode, but for the website, it will be a session id cookie
//! sent by the browser.
//!
//! For each case, the player's role (Player, Mod, Admin) is also checked to correspond
//! to the required role.
//!
//! The procedure to initialize the tokens uses the OAuth 2.0 protocol provided by ManiaPlanet.
//! It has only access to the `basic` scope, meaning the name of the player, his login,
//! and his zone path. We don't use the ManiaPlanet OAuth system to retrieve information about
//! the player, only to associate it with the Obstacle tokens.
//! For now, this procedure is done from the Obstacle Titlepack. It is as follows:
//!
//! 1. The script generates a random string named `state`
//! 2. It sends a POST request to `/player/get_token`, with the corresponding payload
//! 3. It waits a bit, then opens a URL for the player to
//!    <https://prod.live.maniaplanet.com/login/oauth/authorize?response_type=code&client_id=de1ce3ba8e&redirect_uri=https://obstacle.titlepack.io/give_token&state=`state`&scope=basic>
//! 4. The player logs in with his ManiaPlanet account, and is redirected to the page at
//!    <https://obstacle.titlepack.io/give_token> URL.
//! 5. This page executes a JavaScript code that sends a POST request to `/player/give_token`
//!    with the code provided by the ManiaPlanet OAuth system and the same `state`.
//! 6. The authentication system validates the provided code, and generates the 2 tokens
//!    for the player.
//! 7. The POST `/player/give_token` request returns a `200 OK` response with a `Set-Cookie` header
//!    containing the encoded session ID with the website token stored in it bound with the player's login
//! 8. The POST `/player/get_token` request returns the response with the generated ManiaPlanet token
//!    that will be used to authenticate the player in the gamemode.
//!
//! The generated tokens have a time-to-live of 6 months. Passed this time, the authentication system
//! will return an `Unauthorized` error. The gamemode script will have to execute the procedure
//! from above.
//!
//! So in the documentation, the `/player/give_token` endpoint represents the POST request sent by
//! the browser ; while `/player/get_token` is the one sent by Obstacle gamemode.
//!
//! See <https://github.com/maniaplanet/documentation/blob/master/13.web-services/01.oauth2/docs.md#auth-code-flow-or-explicit-flow-or-server-side-flow>
//! for more information.

#[cfg(auth)]
pub mod gen_token;

mod check;
pub use check::check_auth_for;

use records_lib::RedisPool;

use crate::RecordsResultExt as _;
use crate::utils::{ApiStatus, get_api_status};
use crate::{AccessTokenErr, FitRequestId, RecordsError, RecordsResponse, must};
use crate::{RecordsErrorKind, RecordsResult};
use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest};
use chrono::{DateTime, Utc};
use entity::types;
use futures::Future;
use sea_orm::DbConn;
use serde::{Deserialize, Serialize};
use std::future::{Ready, ready};
use std::pin::Pin;
use std::{collections::HashMap, time::Duration};
use tokio::sync::Mutex;
use tokio::sync::oneshot::{self, Receiver, Sender};
use tokio::time::timeout;
use tracing::Level;
use tracing_actix_web::RequestId;

#[allow(dead_code)] // Allow unused flags
pub mod privilege {
    pub type Flags = u8;

    pub const PLAYER: Flags = 0b0001;
    pub const MOD: Flags = 0b0011;
    pub const ADMIN: Flags = 0b1111;
}

pub const WEB_TOKEN_SESS_KEY: &str = "__obs_web_token";

/// The state string expires in 5 minutes.
///
/// This is typically used to set a timeout for the POST /player/get_token request sent by
/// the Obstacle gamemode, waiting for the browser of the player to make the
/// POST /player/give_token request with the same state string.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 5);

/// Represents the current state of a communication between the /player/get_token and
/// /player/give_token endpoints.
#[derive(Debug)]
struct TokenState {
    /// The sender of the /player/get_token endpoint.
    tx: Sender<Message>,
    /// The receiver of the /player/get_token endpoint.
    rx: Receiver<Message>,
    /// The instant where this state has been initialized. This is used to prevent
    /// endpoints to communicate with a `state` string that has already been used for
    /// other endpoints.
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

/// Represents the messages that are exchanged between the /player/get_token and /player/give_token
/// endpoints.
#[derive(Debug)]
#[non_exhaustive]
pub enum Message {
    /// The /player/give_token gives the code provided by the ManiaPlanet OAuth system
    MPCode(String),
    /// After checking for the code provided by the /player/give_token endpoint, the /player/get_token
    /// endpoint checks it, and returned an error
    InvalidMPCode,
    /// The /player/get_token endpoint received the code provided by the /player/give_token endpoint,
    /// sent the corresponding request to ManiaPlanet services, and the latter returned an error.
    AccessTokenErr(AccessTokenErr),
    /// The /player/get_token endpoint received the code from the /player/give_token endpoint, and
    /// has successfuly generated the Obstacle tokens for the player. It sends back the new website
    /// token of the player.
    Ok(WebToken),
}

/// Holds the authentication system state between the endpoints.
///
/// With its internal hash-map, it allows the /player/get_token and /player_give_token endpoints
/// to communicate with each other, by providing the same `state` string.
#[derive(Debug, Default)]
pub struct AuthState {
    states_map: Mutex<HashMap<String, TokenState>>,
}

impl AuthState {
    /// Removes a state from the state map.
    pub async fn remove_state(&self, state: String) {
        let mut state_map = self.states_map.lock().await;
        state_map.remove(&state);
        tracing::event!(
            Level::INFO,
            "Removed state `{}` from the state_map, {} state(s) in total",
            state,
            state_map.len()
        );
    }

    /// Called by the `/player/get_token` endpoint, this method is used to retrieve
    /// the crossed channel used to communicate with the `/player/give_token` endpoint.
    ///
    /// This method should be called before the `/player/give_token` endpoint calls the
    /// [`Self::browser_connected_for`] method.
    ///
    /// # Arguments
    ///
    /// * `state`, the state string, that is the same as the one retrieved by the `/player/give_token`
    ///   endpoint.
    pub async fn connect_with_browser(
        &self,
        state: String,
    ) -> RecordsResult<(Sender<Message>, Receiver<Message>)> {
        // cross channels
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();

        // store channel in the state_map
        {
            let mut state_map = self.states_map.lock().await;
            let state_info = state.clone();
            if let Some(TokenState { instant, .. }) = state_map.get(&state) {
                return Err(RecordsErrorKind::StateAlreadyReceived(*instant));
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

    /// Called by the `/player/give_token` endpoint, this method is used to send the state
    /// and code received from ManiaPlanet OAuth system, to the `/player/get_token` endpoint.
    ///
    /// After a success communication with the other endpoint, the method returns the web token
    /// of the user who has signed in. This is then stored in his session.
    ///
    /// This method should be called after the `/player/get_token` endpoint called the
    /// [`Self::connect_with_browser`] method.
    pub async fn browser_connected_for(
        &self,
        state: String,
        code: String,
    ) -> RecordsResult<WebToken> {
        let mut state_map = self.states_map.lock().await;

        let web_token = if let Some(TokenState { tx, rx, .. }) = state_map.remove(&state) {
            if tx.send(Message::MPCode(code)).is_err() {
                return Err(RecordsErrorKind::MissingGetTokenReq);
            }

            match timeout(TIMEOUT, rx).await {
                Ok(Ok(res)) => match res {
                    Message::Ok(web_token) => web_token,
                    Message::InvalidMPCode => return Err(RecordsErrorKind::InvalidMPCode),
                    Message::AccessTokenErr(err) => {
                        return Err(RecordsErrorKind::AccessTokenErr(err));
                    }
                    _ => unreachable!(),
                },
                _ => {
                    tracing::event!(Level::WARN, "Token state `{}` timed out", state);
                    return Err(RecordsErrorKind::Timeout(TIMEOUT));
                }
            }
        } else {
            return Err(RecordsErrorKind::MissingGetTokenReq);
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

struct ExtAuthHeaders {
    player_login: Option<String>,
    authorization: Option<String>,
}

fn ext_auth_headers(req: &HttpRequest) -> ExtAuthHeaders {
    fn ext_header(req: &HttpRequest, header: &str) -> Option<String> {
        req.headers()
            .get(header)
            .and_then(|h| h.to_str().map(str::to_owned).ok())
    }

    ExtAuthHeaders {
        player_login: ext_header(req, "PlayerLogin"),
        authorization: ext_header(req, "Authorization"),
    }
}

pub struct MPAuthGuard<const ROLE: privilege::Flags = { privilege::PLAYER }> {
    pub login: String,
}

impl<const MIN_ROLE: privilege::Flags> FromRequest for MPAuthGuard<MIN_ROLE> {
    type Error = RecordsError;

    type Future = Pin<Box<dyn Future<Output = RecordsResponse<Self>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        async fn check<const ROLE: privilege::Flags>(
            request_id: RequestId,
            conn: DbConn,
            redis_pool: RedisPool,
            login: Option<String>,
            token: Option<String>,
        ) -> RecordsResponse<MPAuthGuard<ROLE>> {
            let Some(login) = login else {
                return Err(RecordsErrorKind::Unauthorized).fit(request_id);
            };

            let mut redis_conn = redis_pool.get().await.with_api_err().fit(request_id)?;

            check::check_auth_for(&conn, &mut redis_conn, &login, token.as_deref(), ROLE)
                .await
                .fit(request_id)?;

            Ok(MPAuthGuard { login })
        }

        let req_id = must::have_request_id(req);
        let ExtAuthHeaders {
            player_login,
            authorization,
        } = ext_auth_headers(req);

        // FIXME: by extracting sql and redis pools separately, we're cloning each of them twice.
        Box::pin(check(
            req_id,
            must::have_dbconn(req),
            must::have_redis_pool(req),
            player_login,
            authorization,
        ))
    }
}

/// Represents the `Authorization` and `PlayerLogin` headers retrieved from an HTTP request.
#[derive(Clone)]
pub struct AuthHeader {
    pub login: String,
    pub token: String,
}

impl FromRequest for AuthHeader {
    type Error = RecordsError;

    type Future = Ready<RecordsResponse<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let request_id = must::have_request_id(req);

        let ExtAuthHeaders {
            player_login,
            authorization,
        } = ext_auth_headers(req);

        let Some(login) = player_login else {
            return ready(Err(RecordsError {
                request_id,
                kind: RecordsErrorKind::Unauthorized,
            }));
        };

        #[cfg(auth)]
        {
            let Some(token) = authorization else {
                return ready(Err(RecordsError {
                    request_id,
                    kind: RecordsErrorKind::Unauthorized,
                }));
            };

            ready(Ok(Self { login, token }))
        }

        #[cfg(not(auth))]
        ready(Ok(Self {
            login,
            token: authorization.unwrap_or_default(),
        }))
    }
}

/// A guard that checks that the API isn't currently under maintenance.
pub struct ApiAvailable;

impl FromRequest for ApiAvailable {
    type Error = RecordsError;

    type Future = Pin<Box<dyn Future<Output = RecordsResponse<Self>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        async fn check_status(conn: DbConn, req_id: RequestId) -> RecordsResponse<ApiAvailable> {
            match get_api_status(&conn).await.fit(req_id)? {
                ApiStatus {
                    at,
                    kind: types::ApiStatusKind::Maintenance,
                } => Err(RecordsErrorKind::Maintenance(at)).fit(req_id),
                _ => Ok(ApiAvailable),
            }
        }

        let req_id = must::have_request_id(req);
        let conn = must::have_dbconn(req);

        Box::pin(check_status(conn, req_id))
    }
}

/// Represents the information stored in the session cookie of the user sent by the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebToken {
    pub login: String,
    pub token: String,
}
