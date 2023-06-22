//! Module used to serve the routes mainly used by the Obstacle gamemode. Each submodule is
//! specific for a route segment.

use actix_web::web::{JsonConfig, Query};
use actix_web::{web, Scope};
use reqwest::Client;

use crate::utils::format_map_key;
use crate::{
    models::{Map, Player},
    redis,
    utils::json,
    Database, RecordsError, RecordsResult,
};
use actix_web::{web::Data, Responder};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use self::admin::admin_scope;
use self::event::event_scope;
use self::map::map_scope;
use self::player::player_scope;

pub mod admin;
pub mod event;
pub mod map;
pub mod player;

pub fn api_route() -> Scope {
    let json_config = JsonConfig::default().limit(1024 * 16);

    web::scope("")
        .app_data(json_config)
        .app_data(Data::new(Client::new()))
        .route("/overview", web::get().to(overview))
        .service(player_scope())
        .service(map_scope())
        .service(admin_scope())
        .service(event_scope())
}

#[derive(Deserialize)]
struct OverviewQuery {
    #[serde(alias = "playerId")]
    login: String,
    #[serde(alias = "mapId")]
    map_uid: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename = "records")]
pub struct RankedRecord {
    pub rank: u32,
    pub login: String,
    pub nickname: String,
    pub time: i32,
}

async fn append_range(
    db: &Database,
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

    if ids.is_empty() {
        return;
    }

    let params = ids
        .iter()
        .map(|_| "?".to_string())
        .collect::<Vec<String>>()
        .join(",");

    let query = format!(
        "SELECT CAST(RANK() OVER (ORDER BY time ASC) AS UNSIGNED) + ? AS rank,
            players.login AS login,
            players.name AS nickname,
            MIN(time) as time
        FROM records INNER JOIN players ON records.player_id = players.id
        WHERE map_id = ? AND player_id IN ({})
        GROUP BY player_id
        ORDER BY time ASC, record_date ASC",
        params
    );

    let mut query = sqlx::query_as(&query).bind(start).bind(map_id);
    for id in ids {
        query = query.bind(id);
    }

    ranked_records.extend(query.fetch_all(&db.mysql_pool).await.unwrap());
}

async fn overview(db: Data<Database>, body: Query<OverviewQuery>) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

    let Some(Map { id: map_id, .. }) = player::get_map_from_game_id(&db, &body.map_uid).await? else {
        return Err(RecordsError::MapNotFound(body.map_uid));
    };
    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };

    let mut redis_conn = db.redis_pool.get().await.unwrap();

    // Update redis if needed
    let key = format_map_key(map_id);
    let count = redis::update_leaderboard(&db, &key, map_id).await? as u32;

    let mut ranked_records: Vec<RankedRecord> = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: u32 = 15;
    const NO_RECORD_ROWS: u32 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = redis_conn.zrank(&key, player_id).await.unwrap();
    let player_rank = player_rank.map(|r: i64| (r as u64) as u32);

    let mut start: u32 = 0;
    let mut end: u32;

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            append_range(&db, &mut ranked_records, map_id, &key, start, TOTAL_ROWS).await;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            append_range(&db, &mut ranked_records, map_id, &key, start, 3).await;

            // the rest is centered around the player
            let row_minus_top3 = TOTAL_ROWS - 3;
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
        if count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            append_range(
                &db,
                &mut ranked_records,
                map_id,
                &key,
                start,
                NO_RECORD_ROWS - 3,
            )
            .await;

            // last 3
            append_range(&db, &mut ranked_records, map_id, &key, count - 3, count).await;
        }
        // There is enough records to display them all
        else {
            append_range(
                &db,
                &mut ranked_records,
                map_id,
                &key,
                start,
                NO_RECORD_ROWS,
            )
            .await;
        }
    }

    #[derive(Serialize)]
    struct Response {
        response: Vec<RankedRecord>,
    }
    json(Response {
        response: ranked_records,
    })
}
