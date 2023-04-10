use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use actix_multipart::form::{text::Text, MultipartForm};
use actix_web::{
    web::{Data, Json},
    HttpResponse, Responder,
};
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::time::timeout;
use tracing::Level;

use crate::{
    admin,
    auth::{AuthFields, AuthState, ExtractAuthFields, TIMEOUT},
    models::{Banishment, Map, Player, Record, Role},
    redis,
    utils::wrap_xml,
    Database, RecordsError, RecordsResult,
};

#[derive(Deserialize)]
pub struct UpdatePlayerBody {
    pub nickname: String,
    pub country: String,
}

pub async fn update_or_insert(
    db: &Database,
    login: &str,
    body: UpdatePlayerBody,
) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar!("SELECT id FROM players WHERE login = ?", login)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        sqlx::query!(
            "UPDATE players SET name = ?, country = ? WHERE id = ?",
            body.nickname,
            body.country,
            id
        )
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

#[derive(Deserialize, Serialize)]
pub struct CheckpointTime {
    pub cp_num: u32,
    pub time: i32,
}

#[derive(Deserialize, Serialize)]
pub struct HasFinishedBody {
    pub secret: String,
    pub state: String,
    pub time: i32,
    #[serde(alias = "respawnCount")]
    pub respawn_count: i32,
    #[serde(alias = "playerId")]
    pub player_login: String,
    #[serde(alias = "mapId")]
    pub map_game_id: String,
    pub flags: Option<u32>,
    pub cps: Vec<CheckpointTime>,
}

impl ExtractAuthFields for HasFinishedBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.player_login,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename(serialize = "response"))]
pub struct HasFinishedResponse {
    #[serde(rename = "newBest")]
    pub has_improved: bool,
    pub login: String,
    pub old: i32,
    pub new: i32,
}

pub async fn player_finished(
    state: Data<AuthState>,
    db: Data<Database>,
    body: Json<HasFinishedBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Player, &body).await?;
    let inputs_path = state.get_inputs_path(body.state.clone()).await?;
    let finished = inner_player_finished(db, body, inputs_path).await?;
    wrap_xml(&finished)
}

#[derive(MultipartForm)]
pub struct PlayerInputs {
    state: Text<String>,
    inputs: Text<String>,
}

pub async fn register_inputs(
    state: Data<AuthState>,
    body: MultipartForm<PlayerInputs>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    let pre_path = "/var/www/html/inputs";
    let random = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .collect::<Vec<u8>>();
    let random = String::from_utf8(random).expect("random inputs file name not utf8");
    let path = format!("{pre_path}/{}{random}", Utc::now().timestamp());

    state
        .inputs_received_for(body.state.0, path.clone())
        .await?;

    fs::create_dir_all(pre_path)?;
    let mut f = BufWriter::with_capacity(32 * 1024, File::create(path)?);
    for chunk in body.inputs.as_bytes().chunks(10) {
        f.write_all(chunk)?;
    }
    f.flush()?;

    Ok(HttpResponse::NoContent())
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
    db: &actix_web::web::Data<Database>,
    map_game_id: &str,
) -> Result<Option<Map>, RecordsError> {
    let r = sqlx::query_as("SELECT * FROM maps WHERE game_id = ?")
        .bind(map_game_id)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

async fn inner_player_finished(
    db: Data<Database>,
    body: HasFinishedBody,
    inputs_path: String,
) -> RecordsResult<HasFinishedResponse> {
    let Some(Player { id: player_id, .. }) = get_player_from_login(&db, &body.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.player_login));
    };

    if let Some(ban) = check_banned(&db, player_id).await? {
        return Err(RecordsError::BannedPlayer(ban));
    }

    let Some(Map { id: map_id, .. }) = get_map_from_game_id(&db, &body.map_game_id).await? else {
        return Err(RecordsError::MapNotFound(body.map_game_id));
    };

    let mut redis_conn = db.redis_pool.get().await.unwrap();

    let old_record = sqlx::query_as!(
        Record,
        "SELECT * FROM records WHERE map_id = ? AND player_id = ?
            ORDER BY record_date DESC LIMIT 1",
        map_id,
        player_id
    )
    .fetch_optional(&db.mysql_pool)
    .await?;

    let now = Utc::now().naive_utc();

    let (old, new, has_improved) = if let Some(Record { time: old, .. }) = old_record {
        if body.time < old {
            let inputs_expiry = None::<u32>;

            // Update redis record
            let key = format!("l0:{}", body.map_game_id);
            let _added: i64 = redis_conn
                .zadd(&key, player_id, body.time)
                .await
                .unwrap_or(0);
            let _count = redis::update_leaderboard(&db, &key, map_id).await?;

            sqlx::query!("INSERT INTO records (player_id, map_id, time, respawn_count, record_date, flags, inputs_path, inputs_expiry) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    player_id,
                    map_id,
                    body.time,
                    body.respawn_count,
                    now,
                    body.flags,
                    inputs_path,
                    inputs_expiry
                )
                    .execute(&db.mysql_pool)
                    .await?;
        }
        (old, body.time, body.time < old)
    } else {
        let inputs_expiry = None::<u32>;

        sqlx::query!(
                "INSERT INTO records (player_id, map_id, time, respawn_count, record_date, flags, inputs_path, inputs_expiry) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                player_id,
                map_id,
                body.time,
                body.respawn_count,
                now,
                body.flags,
                inputs_path,
                inputs_expiry,
            )
                .execute(&db.mysql_pool)
                .await?;

        (body.time, body.time, true)
    };

    Ok(HasFinishedResponse {
        has_improved,
        login: body.player_login,
        old,
        new,
    })
}

#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

async fn check_mp_token(login: &str, token: String) -> RecordsResult<bool> {
    let client = reqwest::Client::new();

    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
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
    state: Data<AuthState>,
    body: Json<GetTokenBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

    // retrieve access_token from browser redirection
    let (tx, rx) = state.connect_with_browser(body.state.clone()).await?;
    let access_token = match timeout(TIMEOUT, rx).await {
        Ok(Ok(access_token)) => access_token,
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
    if !check_mp_token(&body.login, access_token).await? {
        tx.send("INVALID_TOKEN".to_owned()).expect(err_msg);
        return Err(RecordsError::InvalidMPToken);
    }

    let token = state.gen_token_for(body.login).await;
    tx.send("OK".to_owned()).expect(err_msg);

    wrap_xml(&GetTokenResponse { token })
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
        .body(include_str!("../public/give_token.html"))
}

#[derive(Deserialize)]
pub struct IsBannedBody {
    secret: String,
    login: String,
}

impl ExtractAuthFields for IsBannedBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.login,
        }
    }
}

#[derive(Serialize)]
struct IsBannedResponse {
    banned: bool,
    current_ban: Option<admin::Banishment>,
}

pub async fn is_banned(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<IsBannedBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Player, &body).await?;

    let Some(Player { id: player_id, .. }) = get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

    let current_ban = admin::is_banned(&db, player_id).await?;
    wrap_xml(&IsBannedResponse {
        banned: current_ban.is_some(),
        current_ban,
    })
}

#[derive(Deserialize)]
pub struct InfoBody {
    secret: String,
    login: String,
}

impl ExtractAuthFields for InfoBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.login,
        }
    }
}

#[derive(Serialize, FromRow)]
struct InfoResponse {
    id: u32,
    login: String,
    name: String,
    join_date: Option<chrono::NaiveDateTime>,
    country: String,
    role_name: String,
}

pub async fn info(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<InfoBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Player, &body).await?;

    let Some(info) = sqlx::query_as::<_, InfoResponse>(
        "SELECT *, (SELECT role_name FROM role WHERE id = role) as role_name
        FROM players WHERE login = ?")
    .bind(&body.login)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

    wrap_xml(&info)
}
