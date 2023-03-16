use actix_multipart::form::text::Text;
use actix_multipart::form::MultipartForm;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Write};

use crate::{auth::AuthState, xml::reply};
use actix_web::{
    get, post,
    web::{Data, Json, Query},
    HttpResponse, Responder,
};
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use rand::distributions::Alphanumeric;
use rand::Rng;
use records_lib::{escape::*, HasFinishedResponse, RecordsError};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{mysql, FromRow};

fn wrap_response_xml<T: Serialize>(res: T) -> impl Responder {
    actix_web::dev::Response::ok()
        .map_body(|_, _| reply::xml(&res))
        .map_into_boxed_body()
}

#[derive(Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "mapId")]
    pub map_game_id: String,
    #[serde(alias = "playerId")]
    pub player_login: String,
}

#[derive(Deserialize, Serialize)]
pub struct UpdatePlayerBody {
    pub login: String,
    pub nickname: String,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateMapBody {
    pub name: String,
    #[serde(alias = "maniaplanetMapId")]
    pub map_game_id: String,
    #[serde(alias = "playerId")]
    pub player_login: String,
}

#[derive(Deserialize, Serialize)]
pub struct NewPlayerFinishedBody {
    pub state: String,
    #[serde(flatten)]
    pub finished: HasFinishedBody,
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

#[derive(Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename = "records")]
pub struct RankedRecord {
    pub rank: u32,
    #[serde(rename = "playerId")]
    pub player_login: String,
    pub nickname: String,
    pub time: i32,
}

async fn append_range(
    db: &records_lib::Database,
    ranked_records: &mut Vec<RankedRecord>,
    map_id: u32,
    key: &str,
    start: u32,
    end: u32,
) {
    let mut redis_conn = db.redis_pool.get().await.unwrap();

    // transforms exclusive to inclusive range
    let end = end - 1;
    let ids: Vec<i32> = redis_conn
        .zrange(key, start as isize, end as isize)
        .await
        .unwrap();

    let query = format!(
            "SELECT CAST(0 AS UNSIGNED) as rank, players.login AS player_login, players.name AS nickname, time FROM records INNER JOIN players ON records.player_id = players.id WHERE map_id = ? AND player_id IN ({}) ORDER BY time ASC",
            ids.iter()
                .map(|_| "?".to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
    let mut query = sqlx::query(&query);

    query = query.bind(map_id);
    for id in ids {
        query = query.bind(id);
    }

    let records = query
        .map(|row: mysql::MySqlRow| {
            let mut record = RankedRecord::from_row(&row).unwrap();
            record.nickname = format!("{}", Escape(&record.nickname));
            record
        })
        .fetch_all(&db.mysql_pool)
        .await
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>();

    // transform start from 0-based to 1-based
    let mut rank = start + 1;
    for mut record in records {
        record.rank = rank;
        ranked_records.push(record);
        rank += 1;
    }
}

#[get("/overview")]
pub async fn overview(
    db: Data<records_lib::Database>,
    query: Query<OverviewQuery>,
) -> impl Responder {
    inner_overview_query(db, query).await
}

async fn inner_overview_query(
    db: Data<records_lib::Database>,
    query: Query<OverviewQuery>,
) -> impl Responder {
    let mut redis_conn = db.redis_pool.get().await.unwrap();

    // Insert map and player if they dont exist yet
    let map_id = records_lib::select_or_insert_map(&db, &query.map_game_id).await?;
    let player_id = records_lib::select_or_insert_player(&db, &query.player_login).await?;

    // Update redis if needed
    let key = format!("l0:{}", query.map_game_id);
    let count = records_lib::update_redis_leaderboard(&db, &key, map_id).await? as u32;

    let mut ranked_records: Vec<RankedRecord> = vec![];

    // -- Compute display ranges
    const ROWS: u32 = 15;

    let player_rank: Option<i64> = redis_conn.zrank(&key, player_id).await.unwrap();
    let player_rank = player_rank.map(|r: i64| (r as u64) as u32);

    let mut start: u32 = 0;
    let mut end: u32;

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < ROWS {
            end = ROWS;
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            end = 3;
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;

            // the rest is centered around the player
            let row_minus_top3 = ROWS - 3;
            start = player_rank - row_minus_top3 / 2;
            end = player_rank + row_minus_top3 / 2;
            if end >= count {
                start -= end - count;
                end = count;
            }
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if count > (ROWS - 1) {
            // top (ROWS - 1- 3)
            end = (ROWS - 1) - 3;
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;

            // last 3
            start = count - 3;
            end = count;
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;
        }
        // There is enough records to display them all
        else {
            end = ROWS - 1;
            append_range(&db, &mut ranked_records, map_id, &key, start, end).await;
        }
    }

    Ok::<_, RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml_elements(&ranked_records))
            .map_into_boxed_body(),
    )
}

#[post("/update_player")]
pub async fn update_player(
    db: Data<records_lib::Database>,
    body: Json<UpdatePlayerBody>,
) -> impl Responder {
    inner_update_player(db, body).await
}

async fn inner_update_player(
    db: Data<records_lib::Database>,
    body: Json<UpdatePlayerBody>,
) -> impl Responder {
    let player_id = records_lib::update_player(&db, &body.login, Some(&body.nickname)).await?;
    let mut player = records_lib::select_player(&db, player_id).await?;
    player.name = format!("{}", Escape(&player.name));
    Ok::<_, RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml(&player))
            .map_into_boxed_body(),
    )
}

#[post("/update_map")]
pub async fn update_map(
    db: Data<records_lib::Database>,
    body: Json<UpdateMapBody>,
) -> impl Responder {
    inner_update_map(db, body).await
}

async fn inner_update_map(
    db: Data<records_lib::Database>,
    body: Json<UpdateMapBody>,
) -> impl Responder {
    let map_id = records_lib::update_map(
        &db,
        &body.map_game_id,
        Some(&body.name),
        Some(&body.player_login),
    )
    .await?;
    let mut map = records_lib::select_map(&db, map_id).await?;
    map.name = format!("{}", Escape(&map.name));
    Ok::<_, RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml(&map))
            .map_into_boxed_body(),
    )
}

#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

async fn check_mp_token(login: &str, token: String) -> Result<bool, RecordsError> {
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
pub struct GetTokenResponse {
    token: String,
}

#[post("get_token")]
pub async fn get_token(
    state: Data<AuthState>,
    body: Json<GetTokenBody>,
) -> Result<impl Responder, RecordsError> {
    let body = body.into_inner();

    if check_mp_token(&body.login, body.token).await? {
        let token = state.gen_token_for(body.login).await;
        Ok(wrap_response_xml(GetTokenResponse { token }))
    } else {
        Err(RecordsError::InvalidMPToken)
    }
}

#[post("/player_finished")]
pub async fn player_finished(
    state: Data<AuthState>,
    db: Data<records_lib::Database>,
    body: Json<HasFinishedBody>,
) -> Result<impl Responder, RecordsError> {
    if state
        .check_token_for(&body.secret, &body.player_login)
        .await
    {
        let inputs_path = state.get_inputs_path(body.state.clone()).await?;
        let finished = inner_player_finished(db, body.into_inner(), inputs_path).await?;

        Ok(wrap_response_xml(finished))
    } else {
        Err(RecordsError::Unauthorized)
    }
}

#[derive(MultipartForm)]
pub struct PlayerInputs {
    state: Text<String>,
    inputs: Text<String>,
}

#[post("/player_finished_inputs")]
pub async fn player_finished_inputs(
    state: Data<AuthState>,
    body: MultipartForm<PlayerInputs>,
) -> Result<impl Responder, RecordsError> {
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

    Ok(HttpResponse::Ok())
}

async fn inner_player_finished(
    db: Data<records_lib::Database>,
    body: HasFinishedBody,
    inputs_path: String,
) -> Result<HasFinishedResponse, RecordsError> {
    let Some(player_id) = records_lib::get_player_from_login(&db, &body.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.player_login));
    };

    if let Some(ban) = records_lib::check_banned(&db, player_id).await? {
        return Err(RecordsError::BannedPlayer(ban));
    }

    let Some(map_id) = records_lib::get_map_from_game_id(&db, &body.map_game_id).await? else {
        return Err(RecordsError::MapNotFound(body.map_game_id));
    };

    records_lib::player_new_record(
        &db,
        body.player_login,
        body.map_game_id,
        map_id,
        player_id,
        body.time,
        body.respawn_count,
        body.flags.unwrap_or(0),
        inputs_path,
    )
    .await
}
