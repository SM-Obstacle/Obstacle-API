use std::future::Future;

use crate::{
    auth::{AuthState, SSender, TIMEOUT},
    xml::reply,
};
use actix_web::{
    error::ParseError,
    get,
    http::header::{self, TryIntoHeaderValue},
    post,
    web::{Data, Header, Json, Query},
    HttpMessage, HttpResponse, Responder,
};
use deadpool_redis::redis::AsyncCommands;
use records_lib::{escape::*, RecordsError};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{mysql, FromRow};
use tokio::sync::oneshot;
use tokio::sync::{mpsc::channel, oneshot::error::RecvError};

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
pub struct HasFinishedBody {
    pub time: i32,
    #[serde(alias = "respawnCount")]
    pub respawn_count: i32,
    #[serde(alias = "playerId")]
    pub player_login: String,
    #[serde(alias = "mapId")]
    pub map_game_id: String,
    pub flags: Option<u32>,
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

#[derive(Deserialize, Serialize)]
pub struct NewHasFinishedResponse {
    pub token: String,
    #[serde(flatten)]
    pub finished: HasFinishedResponse,
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
    db: &records_lib::Database, ranked_records: &mut Vec<RankedRecord>, map_id: u32, key: &str,
    start: u32, end: u32,
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
    db: Data<records_lib::Database>, query: Query<OverviewQuery>,
) -> impl Responder {
    inner_overview_query(db, query).await
}

#[get("/api/Records/overview")]
pub async fn overview_compat(
    db: Data<records_lib::Database>, query: Query<OverviewQuery>,
) -> impl Responder {
    inner_overview_query(db, query).await
}

async fn inner_overview_query(
    db: Data<records_lib::Database>, query: Query<OverviewQuery>,
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

    Ok::<_, records_lib::RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml_elements(&ranked_records))
            .map_into_boxed_body(),
    )
}

#[post("/update_player")]
pub async fn update_player(
    db: Data<records_lib::Database>, body: Json<UpdatePlayerBody>,
) -> impl Responder {
    inner_update_player(db, body).await
}

#[post("/api/Players/replaceOrCreate")]
pub async fn update_player_compat(
    db: Data<records_lib::Database>, body: Json<UpdatePlayerBody>,
) -> impl Responder {
    inner_update_player(db, body).await
}

async fn inner_update_player(
    db: Data<records_lib::Database>, body: Json<UpdatePlayerBody>,
) -> impl Responder {
    let player_id = records_lib::update_player(&db, &body.login, Some(&body.nickname)).await?;
    let mut player = records_lib::select_player(&db, player_id).await?;
    player.name = format!("{}", Escape(&player.name));
    Ok::<_, records_lib::RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml(&player))
            .map_into_boxed_body(),
    )
}

#[post("/update_map")]
pub async fn update_map(
    db: Data<records_lib::Database>, body: Json<UpdateMapBody>,
) -> impl Responder {
    inner_update_map(db, body).await
}

#[post("/api/Maps/replaceOrCreate")]
pub async fn update_map_compat(
    db: Data<records_lib::Database>, body: Json<UpdateMapBody>,
) -> impl Responder {
    inner_update_map(db, body).await
}

async fn inner_update_map(
    db: Data<records_lib::Database>, body: Json<UpdateMapBody>,
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
    Ok::<_, records_lib::RecordsError>(
        actix_web::dev::Response::ok()
            .map_body(|_, _| reply::xml(&map))
            .map_into_boxed_body(),
    )
}

pub struct Authorization(String);

impl TryIntoHeaderValue for Authorization {
    type Error = header::InvalidHeaderValue;

    fn try_into_value(self) -> Result<header::HeaderValue, Self::Error> {
        header::HeaderValue::from_str(&self.0)
    }
}

impl header::Header for Authorization {
    fn name() -> header::HeaderName {
        header::HeaderName::from_bytes(b"Authorization").unwrap()
    }

    fn parse<M: HttpMessage>(msg: &M) -> Result<Self, ParseError> {
        let header = msg
            .headers()
            .get(Self::name())
            .ok_or(ParseError::Incomplete)?;
        let value = header.to_str().map_err(|_| ParseError::Header)?;
        Ok(Authorization(value.to_string()))
    }
}

#[derive(Deserialize, Debug)]
struct MPServerRes {
    login: String,
    #[serde(rename = "nickname")]
    _nickname: String,
    #[serde(rename = "path")]
    _path: String,
}

async fn check_mp_token(login: &str, token: &str) -> Result<bool, RecordsError> {
    let client = reqwest::Client::new();

    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;
    let res: MPServerRes = match res.status() {
        StatusCode::OK => res.json().await?,
        _ => return Ok(false),
    };

    Ok(res.login == login)
}

async fn timeout_tx<F>(
    os_rx: F, server_state: &AuthState, state: &str,
) -> Result<SSender, RecordsError>
where
    F: Future<Output = Result<SSender, RecvError>>,
{
    match tokio::time::timeout(TIMEOUT, os_rx).await {
        Ok(Ok(tx)) => Ok(tx),
        Ok(Err(_)) => Err(RecordsError::Timeout),
        Err(_elapsed) => {
            server_state.remove_state(state.to_owned()).await;
            Err(RecordsError::Timeout)
        }
    }
}

/// This is the endpoint called by the Obstacle Titlepack Script when a player finishes a map.
/// 
/// The Script should call this request with a random state, and open a ManiaPlanet page
/// to the player on his browser, with the same state as query parameter. When he logs in,
/// the page will redirect to the /new_token page, which redirects to the /gen_new_token endpoint.
/// 
/// The /gen_new_token and /new_player_finished endpoints will then synchronize, and generate a new
/// token for the player if everything is correct.
/// The generated token for the player is then returned to the Script, which will then store it
/// in the player's profile. This token will be then used as an authorization for the /player_finished
/// request.
/// 
/// Thus, this endpoint should be called before the /gen_new_token endpoint.
#[post("/new_player_finished")]
pub async fn new_player_finished(
    server_state: Data<AuthState>, db: Data<records_lib::Database>,
    body: Json<NewPlayerFinishedBody>,
) -> Result<impl Responder, RecordsError> {
    let body = body.into_inner();
    let state = body.state;

    let (tx, mut rx) = channel(10);
    let tx = {
        let (os_tx, os_rx) = oneshot::channel();
        server_state.insert_state(state.clone(), os_tx, tx).await?;
        timeout_tx(os_rx, &server_state, &state).await?
    };

    tx.send(body.finished.player_login.clone()).await?;

    let token = match rx.recv().await {
        Some(token) => token,
        None => return Err(RecordsError::InvalidMPToken),
    };
    let finished = inner_player_finished(db, body.finished).await?;

    Ok(wrap_response_xml(NewHasFinishedResponse {
        token,
        finished,
    }))
}

#[post("/player_finished")]
pub async fn player_finished(
    authorization: Header<Authorization>, state: Data<AuthState>, db: Data<records_lib::Database>,
    body: Json<HasFinishedBody>,
) -> Result<impl Responder, RecordsError> {
    let token = authorization.into_inner().0;
    if state.check_token_for(&token, &body.player_login).await {
        let finished = inner_player_finished(db, body.into_inner()).await?;
        Ok(wrap_response_xml(finished))
    } else {
        Err(RecordsError::Unauthorized)
    }
}

async fn inner_player_finished(
    db: Data<records_lib::Database>, body: HasFinishedBody,
) -> Result<HasFinishedResponse, RecordsError> {
    let banned_players = ["xxel94toonzxx", "encht"];
    let is_banned = banned_players
        .iter()
        .any(|&banned_player| body.player_login == banned_player);

    // Insert map and player if they dont exist yet
    let map_id = records_lib::select_or_insert_map(&db, &body.map_game_id).await?;
    let player_id = records_lib::select_or_insert_player(&db, &body.player_login).await?;

    if is_banned {
        return Err(RecordsError::BannedPlayer);
    }

    let (old, new) = records_lib::player_new_record(
        &db,
        &body.map_game_id,
        map_id,
        player_id,
        body.time,
        body.respawn_count,
        body.flags.unwrap_or(0),
    )
    .await?;

    Ok(HasFinishedResponse {
        has_improved: old.as_ref().map_or(true, |old| new.time < old.time),
        login: body.player_login,
        old: old.as_ref().map_or(new.time, |old| old.time),
        new: new.time,
    })
}

#[derive(Deserialize)]
struct NewTokenBody {
    #[serde(rename = "token_type")]
    _token_type: String,
    #[serde(rename = "expires_in")]
    _expires_in: String,
    access_token: String,
    state: String,
}

/// This is the endpoint redirected by the /new_token page (with JavaScript).
/// 
/// It synchronizes with the previous /new_player_finished request with the same state.
/// It checks that the given token is valid and then generates a new token for the player.
/// The generated token is sent in response by the /new_player_finished request to the Script.
#[post("gen_new_token")]
async fn gen_new_token(
    state: Data<AuthState>, body: Json<NewTokenBody>,
) -> Result<impl Responder, RecordsError> {
    let body = body.into_inner();

    let (tx, mut rx) = channel(10);
    let tx = {
        let (os_tx, os_rx) = oneshot::channel();
        state.exchange_tx(body.state.clone(), os_tx, tx).await?;
        timeout_tx(os_rx, &state, &body.state).await?
    };

    let login = rx.recv().await.expect("channel disconnected");
    if check_mp_token(&login, &body.access_token).await? {
        let token = state.gen_token_for(login).await;
        tx.send(token).await?;
        state.remove_state(body.state).await;
        Ok(HttpResponse::Ok())
    } else {
        Err(RecordsError::InvalidMPToken)
    }
}

/// This is the endpoint that ManiaPlanet redirects to after the user has logged in
///
/// It will then call the /gen_new_token endpoint to actually generate a new token
/// for the player.
///
/// From the Obstacle Script perspective, this is the endpoint that is called
/// when the player has logged in ManiaPlanet after he finished a map.
#[get("/new_token")]
async fn new_token() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(include_str!("../public/new_token.html"))
}
