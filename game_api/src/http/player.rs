use std::sync::OnceLock;

use actix_session::Session;
use actix_web::{
    web::{self, Data, Json, Query},
    HttpResponse, Responder, Scope,
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, MySqlPool};
use tokio::time::timeout;
use tracing::Level;

use crate::{
    auth::{
        self, privilege, AuthHeader, AuthState, MPAuthGuard, Message, WebToken, TIMEOUT,
        WEB_TOKEN_SESS_KEY,
    },
    models::{Banishment, Map, Player},
    must, read_env_var_file,
    utils::json,
    AccessTokenErr, Database, RecordsError, RecordsResult,
};

use super::{admin, pb, player_finished as pf};

pub fn player_scope() -> Scope {
    web::scope("/player")
        .route("/update", web::post().to(update))
        .route("/finished", web::post().to(finished))
        .route("/get_token", web::post().to(get_token))
        .route("/give_token", web::post().to(post_give_token))
        .route("/pb", web::get().to(pb))
        .route("/times", web::post().to(times))
        .route("/info", web::get().to(info))
}

#[derive(Serialize, Deserialize, Clone, FromRow, Debug)]
pub struct UpdatePlayerBody {
    pub login: String,
    pub name: String,
    pub zone_path: Option<String>,
}

async fn insert_player(db: &Database, login: &str, body: UpdatePlayerBody) -> RecordsResult<u32> {
    let id = sqlx::query_scalar(
        "INSERT INTO players
        (login, name, join_date, zone_path, admins_note, role)
        VALUES (?, ?, SYSDATE(), ?, NULL, 0) RETURNING id",
    )
    .bind(login)
    .bind(body.name)
    .bind(body.zone_path)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
}

pub async fn get_or_insert(
    db: &Database,
    login: &str,
    body: UpdatePlayerBody,
) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM players WHERE login = ?")
        .bind(login)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        return Ok(id);
    }

    insert_player(db, login, body).await
}

pub async fn update(
    db: Data<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<UpdatePlayerBody>,
) -> RecordsResult<impl Responder> {
    match auth::check_auth_for(&db, &login, &token, privilege::PLAYER).await {
        Ok(id) => update_player(&db, id, body).await?,
        Err(RecordsError::PlayerNotFound(_)) => {
            let _ = insert_player(&db, &login, body).await?;
        }
        Err(e) => return Err(e),
    }

    Ok(HttpResponse::Ok().finish())
}

pub async fn update_player(
    db: &Database,
    player_id: u32,
    body: UpdatePlayerBody,
) -> RecordsResult<()> {
    sqlx::query("UPDATE players SET name = ?, zone_path = ? WHERE id = ?")
        .bind(body.name)
        .bind(body.zone_path)
        .bind(player_id)
        .execute(&db.mysql_pool)
        .await?;

    Ok(())
}

pub async fn get_player_from_login(
    db: &Database,
    player_login: &str,
) -> Result<Option<Player>, RecordsError> {
    let r = sqlx::query_as("SELECT * FROM players WHERE login = ?")
        .bind(player_login)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

pub async fn check_banned(
    db: &MySqlPool,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsError> {
    let r = sqlx::query_as("SELECT * FROM current_bans WHERE player_id = ?")
        .bind(player_id)
        .fetch_optional(db)
        .await?;
    Ok(r)
}

pub async fn get_map_from_game_id(
    db: &Database,
    map_game_id: &str,
) -> Result<Option<Map>, RecordsError> {
    let r = sqlx::query_as("SELECT * FROM maps WHERE game_id = ?")
        .bind(map_game_id)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

#[derive(Serialize)]
struct MPAccessTokenBody<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MPAccessTokenResponse {
    AccessToken { access_token: String },
    Error(AccessTokenErr),
}

#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

static MP_APP_CLIENT_ID: OnceLock<String> = OnceLock::new();
static MP_APP_CLIENT_SECRET: OnceLock<String> = OnceLock::new();

fn get_mp_app_client_id() -> &'static str {
    MP_APP_CLIENT_ID.get_or_init(|| read_env_var_file("RECORDS_MP_APP_CLIENT_ID_FILE"))
}

fn get_mp_app_client_secret() -> &'static str {
    MP_APP_CLIENT_SECRET.get_or_init(|| read_env_var_file("RECORDS_MP_APP_CLIENT_SECRET_FILE"))
}

async fn test_access_token(
    client: &Client,
    login: &str,
    ref code: String,
    ref redirect_uri: String,
) -> RecordsResult<bool> {
    let res = client
        .post("https://prod.live.maniaplanet.com/login/oauth2/access_token")
        .form(&MPAccessTokenBody {
            grant_type: "authorization_code",
            client_id: get_mp_app_client_id(),
            client_secret: get_mp_app_client_secret(),
            code,
            redirect_uri,
        })
        .send()
        .await?
        .json()
        .await?;

    let access_token = match res {
        MPAccessTokenResponse::AccessToken { access_token } => access_token,
        MPAccessTokenResponse::Error(err) => return Err(RecordsError::AccessTokenErr(err)),
    };

    check_mp_token(client, login, access_token).await
}

async fn check_mp_token(client: &Client, login: &str, token: String) -> RecordsResult<bool> {
    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .bearer_auth(token)
        .send()
        .await?;
    let MPServerRes { res_login } = match res.status() {
        StatusCode::OK => res.json().await?,
        _ => return Ok(false),
    };

    Ok(res_login.to_lowercase() == login.to_lowercase())
}

async fn finished(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    body: pf::PlayerFinishedBody,
) -> RecordsResult<impl Responder> {
    let res = pf::finished(login, &db, body, None).await?.res;
    json(res)
}

#[derive(Deserialize)]
pub struct GetTokenBody {
    login: String,
    state: String,
    redirect_uri: String,
}

#[derive(Serialize)]
struct GetTokenResponse {
    token: String,
}

pub async fn get_token(
    db: Data<Database>,
    client: Data<Client>,
    state: Data<AuthState>,
    Json(body): Json<GetTokenBody>,
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
            return Err(RecordsError::Timeout);
        }
    };

    let err_msg = "/get_token rx should not be dropped at this point";

    // check access_token and generate new token for player ...
    match test_access_token(&client, &body.login, code, body.redirect_uri).await {
        Ok(true) => (),
        Ok(false) => {
            tx.send(Message::InvalidMPCode).expect(err_msg);
            return Err(RecordsError::InvalidMPCode);
        }
        Err(RecordsError::AccessTokenErr(err)) => {
            tx.send(Message::AccessTokenErr(err.clone()))
                .expect(err_msg);
            return Err(RecordsError::AccessTokenErr(err));
        }
        err => {
            let _ = err?;
        }
    }

    let (mp_token, web_token) = auth::gen_token_for(&db, &body.login).await?;
    tx.send(Message::Ok(WebToken {
        login: body.login,
        token: web_token,
    }))
    .expect(err_msg);

    json(GetTokenResponse { token: mp_token })
}

#[derive(Deserialize)]
pub struct GiveTokenBody {
    code: String,
    state: String,
}

#[derive(Serialize)]
pub struct GiveTokenResponse {
    login: String,
    token: String,
}

pub async fn post_give_token(
    session: Session,
    state: Data<AuthState>,
    Json(body): Json<GiveTokenBody>,
) -> RecordsResult<impl Responder> {
    let web_token = state.browser_connected_for(body.state, body.code).await?;
    session
        .insert(WEB_TOKEN_SESS_KEY, web_token)
        .expect("unable to insert session web token");
    Ok(HttpResponse::Ok().finish())
}

#[derive(Serialize)]
struct IsBannedResponse {
    login: String,
    banned: bool,
    current_ban: Option<admin::Banishment>,
}

async fn pb(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    body: pb::PbReq,
) -> RecordsResult<impl Responder> {
    pb::pb(login, db, body, None).await
}

#[derive(Deserialize)]
struct TimesBody {
    maps_uids: Vec<String>,
}

#[derive(Serialize, FromRow)]
struct TimesResponseItem {
    map_uid: String,
    time: i32,
}

async fn times(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    Json(body): Json<TimesBody>,
) -> RecordsResult<impl Responder> {
    let player = must::have_player(&db, &login).await?;

    let query = format!(
        "SELECT m.game_id AS map_uid, MIN(r.time) AS time
        FROM maps m
        INNER JOIN records r ON r.map_id = m.id
        WHERE r.player_id = ? AND m.game_id IN ({})
        GROUP BY m.id",
        body.maps_uids
            .iter()
            .map(|_| "?".to_owned())
            .collect::<Vec<_>>()
            .join(",")
    );

    let mut query = sqlx::query_as::<_, TimesResponseItem>(&query).bind(player.id);

    for map_uid in body.maps_uids {
        query = query.bind(map_uid);
    }

    let result = query.fetch_all(&db.mysql_pool).await?;
    json(result)
}

#[derive(Deserialize)]
pub struct InfoBody {
    login: String,
}

#[derive(Serialize, FromRow)]
struct InfoResponse {
    id: u32,
    login: String,
    name: String,
    join_date: Option<chrono::NaiveDateTime>,
    zone_path: Option<String>,
    role_name: String,
}

pub async fn info(
    db: Data<Database>,
    Query(body): Query<InfoBody>,
) -> RecordsResult<impl Responder> {
    let Some(info) = sqlx::query_as::<_, InfoResponse>(
        "SELECT *, (SELECT role_name FROM role WHERE id = role) as role_name
        FROM players WHERE login = ?")
    .bind(&body.login)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

    json(info)
}
