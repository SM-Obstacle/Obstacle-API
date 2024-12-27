use actix_web::{web, HttpResponse, Responder, Scope};
use secure_string::SecureString;
use tracing_actix_web::RequestId;

use crate::{
    utils, FitRequestId as _, RecordsErrorKind, RecordsResponse, RecordsResult,
    RecordsResultExt as _, Res,
};

pub fn auth_scope() -> Scope {
    web::scope("/auth")
        .service(
            web::scope("/ingame")
                .route("/request", web::post().to(request_auth))
                .route("/oauth", web::post().to(game_oauth))
                .route("/code", web::post().to(game_validate_code)),
        )
        .service(
            web::scope("/browser")
                .route("/oauth", web::post().to(browser_oauth))
                .route("/code", web::post().to(browser_validate_code)),
        )
}

#[derive(serde::Serialize)]
struct RequestAuthResponse {
    state_id: auth::StateId,
}

async fn request_auth(req_id: RequestId) -> RecordsResponse<impl Responder> {
    let state_id = auth::game::request_auth()
        .await
        .map_err(|_| RecordsErrorKind::Forbidden)
        .fit(req_id)?;

    utils::json(RequestAuthResponse { state_id })
}

#[derive(serde::Deserialize)]
struct GameOauthBody {
    state_id: auth::StateId,
    login: String,
    redirect_uri: String,
}

#[derive(Debug, serde::Deserialize)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

async fn check_mp_token(
    client: &reqwest::Client,
    login: &str,
    token: String,
) -> RecordsResult<bool> {
    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .bearer_auth(token)
        .send()
        .await
        .with_api_err()?;
    let MPServerRes { res_login } = match res.status() {
        reqwest::StatusCode::OK => res.json().await.with_api_err()?,
        _ => return Ok(false),
    };

    Ok(res_login.to_lowercase() == login.to_lowercase())
}

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
    Error(crate::error::AccessTokenErr),
}

async fn check_access_token(
    client: &reqwest::Client,
    login: &str,
    redirect_uri: &str,
    token: SecureString,
) -> RecordsResult<bool> {
    let res = client
        .post("https://prod.live.maniaplanet.com/login/oauth2/access_token")
        .form(&MPAccessTokenBody {
            grant_type: "authorization_code",
            client_id: &crate::env().mp_client_id,
            client_secret: &crate::env().mp_client_secret,
            code: token.unsecure(),
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

async fn game_oauth_impl(client: reqwest::Client, body: GameOauthBody) -> RecordsResult<()> {
    match auth::game::wait_for_oauth(body.state_id, |token| {
        check_access_token(&client, &body.login, &body.redirect_uri, token)
    })
    .await
    {
        Ok(_) => Ok(()),
        Err(auth::game::OAuthErr::InvalidAccessToken(_)) => Err(RecordsErrorKind::InvalidMPCode),
        Err(auth::game::OAuthErr::CheckAccessToken(e)) => Err(e),
        Err(auth::game::OAuthErr::Other(e)) => Err(RecordsErrorKind::from(e)),
    }
}

async fn game_oauth(
    req_id: RequestId,
    client: Res<reqwest::Client>,
    body: web::Json<GameOauthBody>,
) -> RecordsResponse<impl Responder> {
    game_oauth_impl(client.0, body.0)
        .await
        .fit(req_id)
        .map(|_| HttpResponse::NoContent())
}

#[derive(serde::Deserialize)]
struct BrowserOauthBody {
    state_id: auth::StateId,
    data: SecureString,
}

async fn browser_oauth_impl(body: BrowserOauthBody) -> RecordsResult<auth::Code> {
    auth::browser::notify_ingame(body.state_id, body.data)
        .await
        .map_err(|e| match e {
            auth::browser::OAuthErr::InvalidAccessToken(_) => RecordsErrorKind::InvalidMPCode,
            auth::browser::OAuthErr::Other(other) => RecordsErrorKind::from(other),
        })
}

#[derive(serde::Serialize)]
struct BrowserOAuthResponse {
    code: auth::Code,
}

async fn browser_oauth(
    req_id: RequestId,
    body: web::Json<BrowserOauthBody>,
) -> RecordsResponse<impl Responder> {
    let code = browser_oauth_impl(body.0).await.fit(req_id)?;
    utils::json(BrowserOAuthResponse { code })
}

#[derive(serde::Deserialize)]
struct GameValidateCodeBody {
    state_id: auth::StateId,
    code: auth::Code,
}

async fn game_validate_code_impl(body: GameValidateCodeBody) -> RecordsResult<auth::Token> {
    match auth::game::validate_code(body.state_id, body.code).await {
        Ok(token) => Ok(token),
        Err(auth::game::CodeErr::CodeValidation(auth::browser::CodeValidationErr::Failed)) => {
            Err(RecordsErrorKind::Forbidden)
        }
        Err(auth::game::CodeErr::CodeValidation(_)) => Err(RecordsErrorKind::InvalidAuthCode),
        Err(auth::game::CodeErr::Other(other)) => Err(RecordsErrorKind::from(other)),
    }
}

#[derive(serde::Serialize)]
struct GameValidateCodeResponse {
    token: auth::Token,
}

async fn game_validate_code(
    req_id: RequestId,
    body: web::Json<GameValidateCodeBody>,
) -> RecordsResponse<impl Responder> {
    let token = game_validate_code_impl(body.0).await.fit(req_id)?;
    utils::json(GameValidateCodeResponse { token })
}

#[derive(serde::Deserialize)]
struct BrowserValidateCodeBody {
    state_id: auth::StateId,
}

async fn browser_validate_code_impl(body: BrowserValidateCodeBody) -> RecordsResult<()> {
    match auth::browser::wait_code_validation(body.state_id).await {
        Ok(_) => Ok(()),
        Err(auth::browser::CodeErr::Failed) => Err(RecordsErrorKind::Unauthorized),
        Err(auth::browser::CodeErr::Other(other)) => Err(RecordsErrorKind::from(other)),
    }
}

async fn browser_validate_code(
    req_id: RequestId,
    body: web::Json<BrowserValidateCodeBody>,
) -> RecordsResponse<impl Responder> {
    browser_validate_code_impl(body.0)
        .await
        .fit(req_id)
        .map(|_| HttpResponse::NoContent())
}
