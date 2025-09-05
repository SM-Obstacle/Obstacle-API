use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use records_lib::Database;
use reqwest::{Client, StatusCode};
use tokio::time::timeout;
use tracing::Level;

use crate::{
    AccessTokenErr, RecordsErrorKind, RecordsResult, RecordsResultExt as _, Res,
    auth::{self, ApiAvailable, Message, TIMEOUT, WEB_TOKEN_SESS_KEY, WebToken},
    internal,
    utils::json,
};

#[derive(serde::Serialize)]
struct MPAccessTokenBody<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum MPAccessTokenResponse {
    AccessToken { access_token: String },
    Error(AccessTokenErr),
}

#[derive(serde::Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

async fn test_access_token(
    client: &Client,
    login: &str,
    code: &str,
    redirect_uri: &str,
) -> RecordsResult<bool> {
    let res = client
        .post("https://prod.live.maniaplanet.com/login/oauth2/access_token")
        .form(&MPAccessTokenBody {
            grant_type: "authorization_code",
            client_id: &crate::env().mp_client_id,
            client_secret: &crate::env().mp_client_secret,
            code,
            redirect_uri,
        })
        .send()
        .await
        .with_api_err()?
        .json()
        .await
        .with_api_err()?;

    let access_token = match res {
        MPAccessTokenResponse::AccessToken { access_token } => access_token,
        MPAccessTokenResponse::Error(err) => return Err(RecordsErrorKind::AccessTokenErr(err)),
    };

    check_mp_token(client, login, access_token).await
}

async fn check_mp_token(client: &Client, login: &str, token: String) -> RecordsResult<bool> {
    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .bearer_auth(token)
        .send()
        .await
        .with_api_err()?;
    let MPServerRes { res_login } = match res.status() {
        StatusCode::OK => res.json().await.with_api_err()?,
        _ => return Ok(false),
    };

    Ok(res_login.to_lowercase() == login.to_lowercase())
}
#[derive(serde::Deserialize)]
pub struct GetTokenBody {
    login: String,
    state: String,
    redirect_uri: String,
}

pub async fn get_token(
    _: ApiAvailable,
    db: Res<Database>,
    Res(client): Res<Client>,
    state: web::Data<crate::AuthState>,
    web::Json(body): web::Json<GetTokenBody>,
) -> RecordsResult<impl Responder> {
    // retrieve access_token from browser redirection
    let (tx, rx) = state.connect_with_browser(body.state.clone()).await?;
    let code = match timeout(TIMEOUT, rx).await {
        Ok(Ok(Message::MPCode(access_token))) => access_token,
        _ => {
            tracing::event!(
                Level::WARN,
                "Token state `{}` timed out, removing it",
                body.state.clone()
            );
            state.remove_state(body.state).await;
            return Err(RecordsErrorKind::Timeout(TIMEOUT));
        }
    };

    let err_msg = "/get_token rx should not be dropped at this point";

    // check access_token and generate new token for player ...
    match test_access_token(&client, &body.login, &code, &body.redirect_uri).await {
        Ok(true) => (),
        Ok(false) => {
            tx.send(Message::InvalidMPCode)
                .map_err(|_| internal!("{err_msg}"))?;
            return Err(RecordsErrorKind::InvalidMPCode);
        }
        Err(RecordsErrorKind::AccessTokenErr(err)) => {
            tx.send(Message::AccessTokenErr(err.clone()))
                .map_err(|_| internal!("{err_msg}"))?;
            return Err(RecordsErrorKind::AccessTokenErr(err));
        }
        Err(e) => return Err(e),
    }

    let mut redis_conn = db.redis_pool.get().await.with_api_err()?;

    let (mp_token, web_token) =
        auth::gen_token::gen_token_for(&mut redis_conn, &body.login).await?;
    tx.send(Message::Ok(WebToken {
        login: body.login,
        token: web_token,
    }))
    .map_err(|_| internal!("{err_msg}"))?;

    json(super::GetTokenResponse { token: mp_token })
}

#[derive(serde::Deserialize)]
pub struct GiveTokenBody {
    code: String,
    state: String,
}

pub async fn post_give_token(
    session: Session,
    state: web::Data<crate::AuthState>,
    web::Json(body): web::Json<GiveTokenBody>,
) -> RecordsResult<impl Responder> {
    let web_token = state.browser_connected_for(body.state, body.code).await?;

    session
        .insert(WEB_TOKEN_SESS_KEY, web_token)
        .map_err(|err| internal!("unable to insert session web token: {err}"))?;

    Ok(HttpResponse::Ok().finish())
}
