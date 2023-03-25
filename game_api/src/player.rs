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

use crate::{
    auth::AuthState,
    models::{Banishment, Record},
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
        println!("player exists, we update");

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

    println!("player doesnt exist, we insert");

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
    if state
        .check_token_for(&body.secret, &body.player_login)
        .await
    {
        let inputs_path = state.get_inputs_path(body.state.clone()).await?;
        let finished = inner_player_finished(db, body.into_inner(), inputs_path).await?;
        wrap_xml(&finished)
    } else {
        Err(RecordsError::Unauthorized)
    }
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
) -> Result<Option<u32>, RecordsError> {
    let r = sqlx::query_scalar!("SELECT id FROM players WHERE login = ?", player_login)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

pub async fn check_banned(
    db: &Database,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsError> {
    let r = sqlx::query_as::<_, Banishment>(
        "SELECT * FROM banishments
        WHERE player_id = ? AND (SYSDATE() < date_ban + duration OR duration = -1)",
    )
    .bind(player_id)
    .fetch_optional(&db.mysql_pool)
    .await?;
    Ok(r)
}

pub async fn get_map_from_game_id(
    db: &actix_web::web::Data<Database>,
    map_game_id: &str,
) -> Result<Option<u32>, RecordsError> {
    let r = sqlx::query_scalar!("SELECT id FROM maps WHERE game_id = ?", map_game_id)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

async fn inner_player_finished(
    db: Data<Database>,
    body: HasFinishedBody,
    inputs_path: String,
) -> RecordsResult<HasFinishedResponse> {
    let Some(player_id) = get_player_from_login(&db, &body.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.player_login));
    };

    if let Some(ban) = check_banned(&db, player_id).await? {
        return Err(RecordsError::BannedPlayer(ban));
    }

    let Some(map_id) = get_map_from_game_id(&db, &body.map_game_id).await? else {
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
        .get("https://prod.live.maniaplanet.com/ingame/auth")
        .header(
            "Maniaplanet-Auth",
            format!(r#"Login="{login}", Token="{token}""#),
        )
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
    token: String,
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

    if check_mp_token(&body.login, body.token).await? {
        let token = state.gen_token_for(body.login).await;
        wrap_xml(&GetTokenResponse { token })
    } else {
        Err(RecordsError::InvalidMPToken)
    }
}
