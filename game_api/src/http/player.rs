use std::sync::OnceLock;

use actix_session::Session;
use actix_web::{
    web::{self, Data, Json, Query},
    HttpResponse, Responder, Scope,
};
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::time::timeout;
use tracing::Level;

use crate::{
    auth::{self, AuthHeader, AuthState, Message, WebToken, TIMEOUT, WEB_TOKEN_SESS_KEY},
    models::{Banishment, Map, Player, Record, Role},
    redis,
    utils::{format_map_key, json, read_env_var_file},
    Database, RecordsError, RecordsResult,
};

use super::admin;

pub fn player_scope() -> Scope {
    web::scope("/player")
        .route("/update", web::post().to(update))
        .route("/finished", web::post().to(finished))
        .route("/get_token", web::post().to(get_token))
        .service(
            web::resource("/give_token")
                .route(web::post().to(post_give_token))
                .route(web::get().to(get_give_token)),
        )
        .route("/times", web::post().to(times))
        .route("/info", web::get().to(info))
}

#[derive(Serialize, Deserialize, Clone, FromRow)]
pub struct UpdatePlayerBody {
    pub login: String,
    pub name: String,
    pub zone_path: Option<String>,
}

async fn insert_player(db: &Database, body: UpdatePlayerBody) -> RecordsResult<u32> {
    let id = sqlx::query_scalar(
        "INSERT INTO players
        (login, name, join_date, zone_path, admins_note, role)
        VALUES (?, ?, SYSDATE(), ?, NULL, 1) RETURNING id",
    )
    .bind(body.login)
    .bind(body.name)
    .bind(body.zone_path)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
}

pub async fn get_or_insert(db: &Database, body: UpdatePlayerBody) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM players WHERE login = ?")
        .bind(&body.login)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        return Ok(id);
    }

    insert_player(db, body).await
}

pub async fn update(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<UpdatePlayerBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

    match auth::check_auth_for(&db, auth, Role::Player).await {
        Ok(()) => update_or_insert(&db, body).await?,
        Err(RecordsError::PlayerNotFound(_)) => {
            let _ = insert_player(&db, body).await?;
        }
        Err(e) => return Err(e),
    }

    Ok(HttpResponse::Ok().finish())
}

pub async fn update_or_insert(db: &Database, body: UpdatePlayerBody) -> RecordsResult<()> {
    if let Some(id) = sqlx::query_scalar::<_, u32>("SELECT id FROM players WHERE login = ?")
        .bind(&body.login)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        sqlx::query("UPDATE players SET name = ?, zone_path = ? WHERE id = ?")
            .bind(body.name)
            .bind(body.zone_path)
            .bind(id)
            .execute(&db.mysql_pool)
            .await?;

        return Ok(());
    }

    let _ = insert_player(db, body).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct HasFinishedBody {
    pub time: i32,
    pub respawn_count: i32,
    pub login: String,
    pub map_uid: String,
    pub flags: Option<u32>,
    pub cps: Vec<i32>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename(serialize = "response"))]
pub struct HasFinishedResponse {
    pub has_improved: bool,
    pub login: String,
    pub old: i32,
    pub new: i32,
}

pub async fn finished(
    auth: AuthHeader,
    db: Data<Database>,
    body: Json<HasFinishedBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Player).await?;

    let body = body.into_inner();

    let finished = player_finished(db, body).await?;
    json(finished)
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
    db: &Database,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsError> {
    let r = sqlx::query_as(
        "SELECT * FROM banishments
        WHERE player_id = ? AND (date_ban + INTERVAL duration SECOND > NOW() OR duration IS NULL)",
    )
    .bind(player_id)
    .fetch_optional(&db.mysql_pool)
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

async fn player_finished(
    db: Data<Database>,
    body: HasFinishedBody,
) -> RecordsResult<HasFinishedResponse> {
    let Some(Player { id: player_id, .. }) = get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };
    let Some(Map { id: map_id, cps_number, .. }) = get_map_from_game_id(&db, &body.map_uid).await? else {
        return Err(RecordsError::MapNotFound(body.map_uid));
    };

    if matches!(cps_number, Some(num) if num + 1 != body.cps.len() as u32)
        || body.cps.iter().sum::<i32>() != body.time
    {
        return Err(RecordsError::InvalidTimes);
    }

    let mut redis_conn = db.redis_pool.get().await.unwrap();

    let old_record = sqlx::query_as::<_, Record>(
        "SELECT * FROM records WHERE map_id = ? AND player_id = ?
            ORDER BY record_date DESC LIMIT 1",
    )
    .bind(map_id)
    .bind(player_id)
    .fetch_optional(&db.mysql_pool)
    .await?;

    async fn insert_record(
        db: &Database,
        redis_conn: &mut deadpool_redis::Connection,
        player_id: u32,
        map_id: u32,
        body: &HasFinishedBody,
    ) -> RecordsResult<()> {
        let key = format_map_key(map_id);

        let added: Option<i64> = redis_conn.zadd(&key, player_id, body.time).await.ok();
        if added.is_none() {
            let _count = redis::update_leaderboard(db, &key, map_id).await?;
        }

        let now = Utc::now().naive_utc();

        let record_id: u32 = sqlx::query_scalar(
            "INSERT INTO records (player_id, map_id, time, respawn_count, record_date, flags)
            VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(player_id)
        .bind(map_id)
        .bind(body.time)
        .bind(body.respawn_count)
        .bind(now)
        .bind(body.flags)
        .fetch_one(&db.mysql_pool)
        .await?;

        let cps_times = body
            .cps
            .iter()
            .enumerate()
            .map(|(i, t)| format!("({i}, {map_id}, {record_id}, {t})"))
            .collect::<Vec<String>>()
            .join(", ");

        sqlx::query(
            format!(
                "INSERT INTO checkpoint_times (cp_num, map_id, record_id, time)
                VALUES {cps_times}"
            )
            .as_str(),
        )
        .execute(&db.mysql_pool)
        .await?;

        Ok(())
    }

    let (old, new, has_improved) = if let Some(Record { time: old, .. }) = old_record {
        if body.time < old {
            insert_record(&db, &mut redis_conn, player_id, map_id, &body).await?;
        }
        (old, body.time, body.time < old)
    } else {
        insert_record(&db, &mut redis_conn, player_id, map_id, &body).await?;
        (body.time, body.time, true)
    };

    Ok(HasFinishedResponse {
        has_improved,
        login: body.login,
        old,
        new,
    })
}

#[derive(Serialize)]
struct MPAccessTokenBody<'a> {
    grant_type: String,
    client_id: &'a str,
    client_secret: &'a str,
    code: String,
    redirect_uri: String,
}

#[derive(Deserialize)]
struct MPAccessTokenResponse {
    access_token: String,
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
    code: String,
    redirect_uri: String,
) -> RecordsResult<bool> {
    let MPAccessTokenResponse { access_token } = client
        .post("https://prod.live.maniaplanet.com/login/oauth2/access_token")
        .form(&MPAccessTokenBody {
            grant_type: "authorization_code".to_owned(),
            client_id: get_mp_app_client_id(),
            client_secret: get_mp_app_client_secret(),
            code,
            redirect_uri,
        })
        .send()
        .await?
        .json()
        .await?;

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

    Ok(res_login == login)
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
    body: Json<GetTokenBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

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
    if !test_access_token(&client, &body.login, code, body.redirect_uri).await? {
        tx.send(Message::InvalidMPCode).expect(err_msg);
        return Err(RecordsError::InvalidMPCode);
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
    body: Json<GiveTokenBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    let web_token = state.browser_connected_for(body.state, body.code).await?;
    session
        .insert(WEB_TOKEN_SESS_KEY, web_token)
        .expect("unable to insert session web token");
    Ok(HttpResponse::Ok().finish())
}

pub async fn get_give_token() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(include_str!("../../public/give_token.html"))
}

#[derive(Serialize)]
struct IsBannedResponse {
    login: String,
    banned: bool,
    current_ban: Option<admin::Banishment>,
}

#[derive(Deserialize)]
struct TimesBody {
    login: String,
    maps_uids: Vec<String>,
}

#[derive(Serialize, FromRow)]
struct TimesResponseItem {
    map_uid: String,
    time: i32,
}

async fn times(
    auth: AuthHeader,
    db: Data<Database>,
    body: Json<TimesBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Player).await?;

    let body = body.into_inner();

    let Some(player) = get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

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

pub async fn info(db: Data<Database>, body: Query<InfoBody>) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

    let Some(info) = sqlx::query_as::<_, InfoResponse>(
        "SELECT *, (SELECT role_name FROM role WHERE id = role) as role_name
        FROM players WHERE login = ?")
    .bind(&body.login)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

    json(info)
}
