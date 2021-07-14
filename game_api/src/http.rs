use crate::xml;
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{mysql, FromRow};
use std::vec::Vec;
use warp::Filter;

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

pub fn warp_routes(
    db: records_lib::Database,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let db_filter = warp::any().map(move || db.clone());

    // GET /overview?mapId=XXXXX&playerId=XXXXXX
    let new_overview = warp::get()
        .and(warp::path("overview"))
        .and(db_filter.clone())
        .and(warp::query::<OverviewQuery>())
        .and_then(overview)
        .with(warp::trace::named("overview"));

    // POST /update_player  {"login": "smokegun","nickname": "looser"}
    let new_update_player = warp::post()
        .and(warp::path("update_player"))
        .and(db_filter.clone())
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(update_player)
        .with(warp::trace::named("update_player"));

    // POST /update_map  {"name": "XXX","maniaplanetMapId": "XXX", "playerId": "XXX"}
    let new_update_map = warp::post()
        .and(warp::path("update_map"))
        .and(db_filter.clone())
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(update_map)
        .with(warp::trace::named("update_map"));

    // POST player-finished {"time": 10, "respawnCount": 0, "playerId": "smokegun", "mapId": "XXX"}
    let new_player_finished = warp::post()
        .and(warp::path("player-finished"))
        .and(db_filter.clone())
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(player_finished)
        .with(warp::trace::named("player-finished"));


    let compat_prefix = warp::path("api");

    // GET /api/Records/overview?mapId=XXXXX&playerId=XXXXXX
    let compat_overview = warp::get()
        .and(warp::path("Records"))
        .and(warp::path("overview"))
        .and(db_filter.clone())
        .and(warp::query::<OverviewQuery>())
        .and_then(overview)
        .with(warp::trace::named("overview"));

    // POST /api/Players/replaceOrCreate  {"login": "smokegun","nickname": "looser"}
    let compat_update_player = warp::post()
        .and(warp::path("Players"))
        .and(warp::path("replaceOrCreate"))
        .and(db_filter.clone())
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(update_player)
        .with(warp::trace::named("update_player"));

    // POST /api/Maps/replaceOrCreate  {"name": "XXX","maniaplanetMapId": "XXX", "playerId": "XXX"}
    let compat_update_map = warp::post()
        .and(warp::path("Maps"))
        .and(warp::path("replaceOrCreate"))
        .and(db_filter.clone())
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(update_map)
        .with(warp::trace::named("update_map"));

    // POST /api/Records/player-finished {"time": 10, "respawnCount": 0, "playerId": "smokegun", "mapId": "XXX"}
    let compat_player_finished = warp::post()
        .and(warp::path("Records"))
        .and(warp::path("player-finished"))
        .and(db_filter)
        // Only accept bodies smaller than 16kb...
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(player_finished)
        .with(warp::trace::named("player-finished"));

    let compat_routes = compat_prefix.and(
        compat_overview.or(compat_update_player).or(compat_update_map).or(compat_player_finished)
    );

    new_overview
        .or(new_update_player)
        .or(new_update_map)
        .or(new_player_finished)
        .or(compat_routes)
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
        .map(|row: mysql::MySqlRow| RankedRecord::from_row(&row).unwrap())
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

async fn overview(
    db: records_lib::Database, query: OverviewQuery,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut redis_conn = db.redis_pool.get().await.unwrap();

    // Insert map and player if they dont exist yet
    let map_id = records_lib::update_map(&db, &query.map_game_id, None, None).await?;
    let player_id = records_lib::select_or_insert_player(&db, &query.player_login).await?;

    // Update redis if needed
    let key = format!("l0:{}", query.map_game_id);
    dbg!(&key);
    let count = records_lib::update_redis_leaderboard(&db, &key, map_id).await? as u32;

    let mut ranked_records: Vec<RankedRecord> = vec![];

    // -- Compute display ranges
    const ROWS: u32 = 15;

    let player_rank: Option<i64> = redis_conn.zrank(&key, player_id).await.unwrap();
    let player_rank = player_rank.map(|r: i64| (r as u64) as u32);

    let mut start: u32 = 0;
    let mut end: u32;

    if let Some(player_rank) = dbg!(player_rank) {
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

    Ok(xml::reply::xml_elements(&ranked_records))
}

async fn update_player(
    db: records_lib::Database, body: UpdatePlayerBody,
) -> Result<impl warp::Reply, warp::Rejection> {
    let player_id = records_lib::update_player(&db, &body.login, Some(&body.nickname)).await?;
    let player = records_lib::select_player(&db, player_id).await?;
    Ok(xml::reply::xml(&player))
}

async fn select_player(
    db: records_lib::Database, player_id: u32,
) -> Result<impl warp::Reply, warp::Rejection> {
    let player = records_lib::select_player(&db, player_id).await?;
    Ok(xml::reply::xml(&player))
}

async fn update_map(
    db: records_lib::Database, body: UpdateMapBody,
) -> Result<impl warp::Reply, warp::Rejection> {
    let map_id = records_lib::update_map(
        &db,
        &body.map_game_id,
        Some(&body.name),
        Some(&body.player_login),
    )
    .await?;
    let map = records_lib::select_map(&db, map_id).await?;
    Ok(xml::reply::xml(&map))
}

async fn select_map(
    db: records_lib::Database, map_id: u32,
) -> Result<impl warp::Reply, warp::Rejection> {
    let map = records_lib::select_map(&db, map_id).await?;
    Ok(xml::reply::xml(&map))
}

async fn player_finished(
    db: records_lib::Database, body: HasFinishedBody,
) -> Result<impl warp::Reply, warp::Rejection> {
    let banned_players = ["xxel94toonzxx", "encht"];
    let is_banned = banned_players
        .iter()
        .any(|&banned_player| body.player_login == banned_player);

    // Insert map and player if they dont exist yet
    let map_id = records_lib::update_map(&db, &body.map_game_id, None, None).await?;
    let player_id = records_lib::select_or_insert_player(&db, &body.player_login).await?;

    if is_banned {
        return Ok(xml::reply::xml(&HasFinishedResponse {
            has_improved: false,
            login: body.player_login,
            old: body.time,
            new: body.time,
        }));
    }

    let (old, new) = records_lib::player_new_record(
        &db,
        &body.map_game_id,
        map_id,
        player_id,
        body.time,
        body.respawn_count,
        body.flags.unwrap_or(0)
    )
    .await?;

    Ok(xml::reply::xml(&HasFinishedResponse {
        has_improved: old.as_ref().map_or(true, |old| new.time < old.time),
        login: body.player_login,
        old: old.as_ref().map_or(new.time, |old| old.time),
        new: new.time,
    }))
}
