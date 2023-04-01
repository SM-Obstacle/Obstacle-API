use actix_web::web::JsonConfig;
use actix_web::{web, Scope};

use crate::auth::{AuthFields, ExtractAuthFields};
use crate::map::UpdateMapBody;
use crate::models::Role;
use crate::player::UpdatePlayerBody;
use crate::utils::{escaped, wrap_xml, xml_seq};
use crate::{admin, map, player, redis, AuthState, Database, RecordsResult};
use actix_web::{
    web::{Data, Json},
    Responder,
};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{mysql, FromRow};

fn player_scope() -> Scope {
    web::scope("/player")
        .route("/is_banned", web::get().to(player::is_banned))
        .route("/finished", web::post().to(player::player_finished))
        .route("/register_inputs", web::post().to(player::register_inputs))
        .route("/get_token", web::post().to(player::get_token))
        .route("/info", web::get().to(player::info))
}

fn admin_scope() -> Scope {
    web::scope("/admin")
        .route("/del_note", web::post().to(admin::del_note))
        .route("/set_role", web::post().to(admin::set_role))
        .route("/banishments", web::get().to(admin::banishments))
        .route("/ban", web::post().to(admin::ban))
        .route("/unban", web::post().to(admin::unban))
        .route("/player_note", web::get().to(admin::player_note))
}

fn map_scope() -> Scope {
    web::scope("/map")
        .route("/player_rating", web::get().to(map::player_rating))
        .route("/ratings", web::get().to(map::ratings))
        .route("/rating", web::get().to(map::rating))
        .route("/rate", web::post().to(map::rate))
        .route("/reset_ratings", web::post().to(map::reset_ratings))
}

pub fn api_route(db: Database) -> Scope {
    let json_config = JsonConfig::default().limit(1024 * 16);

    web::scope("")
        .app_data(json_config)
        .app_data(Data::new(db))
        .route("/update", web::post().to(update))
        .service(player_scope())
        .service(map_scope())
        .service(admin_scope())
}

#[derive(Deserialize)]
struct UpdateBody {
    secret: String,
    login: String,
    player: UpdatePlayerBody,
    map: UpdateMapBody,
}

impl ExtractAuthFields for UpdateBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.login,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename = "records")]
pub struct RankedRecord {
    pub rank: u32,
    #[serde(rename = "playerId")]
    pub player_login: String,
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
        "SELECT CAST(0 AS UNSIGNED) as rank,
            players.login AS player_login,
            players.name AS nickname,
            time
        FROM records INNER JOIN players ON records.player_id = players.id
        WHERE map_id = ? AND player_id IN ({})
        ORDER BY time ASC",
        params
    );

    let mut query = sqlx::query(&query);

    query = query.bind(map_id);
    for id in ids {
        query = query.bind(id);
    }

    let records = query
        .map(|row: mysql::MySqlRow| {
            let mut record = RankedRecord::from_row(&row).unwrap();
            record.nickname = escaped(&record.nickname);
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

async fn update(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<UpdateBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Player, &body).await?;

    // Insert map and player if they dont exist yet
    let map_id = map::get_or_insert(&db, &body.map).await?;
    let player_id = player::update_or_insert(&db, &body.login, body.player).await?;

    let mut redis_conn = db.redis_pool.get().await.unwrap();

    // Update redis if needed
    let key = format!("l0:{}", body.map.map_uid);
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

    wrap_xml(&xml_seq(Some("response"), &ranked_records)?)
}
