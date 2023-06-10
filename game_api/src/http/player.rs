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
    auth::{self, AuthHeader, AuthState, Message, TIMEOUT},
    models::{Banishment, Map, Player, Record, Role},
    redis,
    utils::{format_map_key, json},
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
        .route("/info", web::get().to(info))
}

#[derive(Deserialize)]
pub struct UpdatePlayerBody {
    pub login: String,
    pub nickname: String,
    pub country: Option<String>,
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

    let id = sqlx::query_scalar(
        "INSERT INTO players
        (login, name, join_date, country, admins_note, role)
        VALUES (?, ?, SYSDATE(), ?, NULL, 1) RETURNING id",
    )
    .bind(login)
    .bind(body.nickname)
    .bind(body.country)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
}

pub async fn update(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<UpdatePlayerBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Player).await?;

    let body = body.into_inner();
    let login = body.login.clone();

    let player_id = update_or_insert(&db, &body.login.clone(), body).await?;
    let current_ban = admin::is_banned(&db, player_id).await?;

    json(IsBannedResponse {
        login,
        banned: current_ban.is_some(),
        current_ban,
    })
}

pub async fn update_or_insert(
    db: &Database,
    login: &str,
    body: UpdatePlayerBody,
) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM players WHERE login = ?")
        .bind(login)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        sqlx::query("UPDATE players SET name = ?, country = ? WHERE id = ?")
            .bind(body.nickname)
            .bind(body.country)
            .bind(id)
            .execute(&db.mysql_pool)
            .await?;

        return Ok(id);
    }

    let id = sqlx::query_scalar(
        "INSERT INTO players
        (login, name, join_date, country, admins_note, role)
        VALUES (?, ?, SYSDATE(), ?, NULL, 1) RETURNING id",
    )
    .bind(login)
    .bind(body.nickname)
    .bind(body.country)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
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
    let Some(Map { id: map_id, .. }) = get_map_from_game_id(&db, &body.map_uid).await? else {
        return Err(RecordsError::MapNotFound(body.map_uid));
    };

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

#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
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
    let access_token = match timeout(TIMEOUT, rx).await {
        Ok(Ok(Message::MPAccessToken(access_token))) => access_token,
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
    if !check_mp_token(&client, &body.login, access_token).await? {
        tx.send(Message::InvalidMPToken).expect(err_msg);
        return Err(RecordsError::InvalidMPToken);
    }

    let token = auth::gen_token_for(&db, body.login).await?;
    tx.send(Message::Ok).expect(err_msg);

    json(GetTokenResponse { token })
}

#[derive(Deserialize)]
pub struct GiveTokenBody {
    access_token: String,
    state: String,
}

pub async fn post_give_token(
    state: Data<AuthState>,
    body: Json<GiveTokenBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state
        .browser_connected_for(body.state, body.access_token)
        .await?;
    Ok(HttpResponse::Ok())
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
pub struct InfoBody {
    login: String,
}

#[derive(Serialize, FromRow)]
struct InfoResponse {
    id: u32,
    login: String,
    name: String,
    join_date: Option<chrono::NaiveDateTime>,
    country: Option<String>,
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
