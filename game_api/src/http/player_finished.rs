use actix_web::web::Json;
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::{
    graphql::get_rank_or_full_update,
    models::{self, Map, Record},
    must, redis,
    utils::format_map_key,
    Database, RecordsError, RecordsResult,
};

use super::event;

#[derive(Deserialize, Debug)]
pub struct HasFinishedBody {
    pub time: i32,
    pub respawn_count: i32,
    pub map_uid: String,
    pub flags: Option<u32>,
    pub cps: Vec<i32>,
}

pub type PlayerFinishedBody = Json<HasFinishedBody>;

#[derive(Deserialize, Serialize)]
pub struct HasFinishedResponse {
    has_improved: bool,
    login: String,
    old: i32,
    new: i32,
    current_rank: i32,
    reversed: bool,
}

// TODO: group parameters
#[allow(clippy::too_many_arguments)]
async fn insert_record(
    db: &Database,
    redis_conn: &mut deadpool_redis::Connection,
    player_id: u32,
    map_id: u32,
    body: &HasFinishedBody,
    key: &str,
    reversed: bool,
    event: Option<&(models::Event, models::EventEdition)>,
) -> RecordsResult<u32> {
    let added: Option<i64> = redis_conn.zadd(key, player_id, body.time).await.ok();
    if added.is_none() {
        let _count = redis::update_leaderboard(db, key, map_id, reversed, event).await?;
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

    Ok(record_id)
}

pub struct FinishedOutput {
    pub record_id: u32,
    pub res: HasFinishedResponse,
}

pub async fn finished(
    login: String,
    db: &Database,
    Json(body): Json<HasFinishedBody>,
    event: Option<&(models::Event, models::EventEdition)>,
) -> RecordsResult<FinishedOutput> {
    // First, we retrieve all what we need to save the record
    let player_id = must::have_player(db, &login).await?.id;
    let Map {
        id: map_id,
        cps_number,
        reversed,
        ..
    } = must::have_map(db, &body.map_uid).await?;
    let reversed = reversed.unwrap_or(false);
    let map_key = format_map_key(map_id, event);

    let (join_event, and_event) = event
        .is_some()
        .then(event::get_sql_fragments)
        .unwrap_or_default();

    // We check that the cps times are coherent to the final time
    if matches!(cps_number, Some(num) if num + 1 != body.cps.len() as u32)
        || body.cps.iter().sum::<i32>() != body.time
    {
        return Err(RecordsError::InvalidTimes);
    }

    let mut redis_conn = db.redis_pool.get().await?;

    let query = format!(
        "SELECT r.* FROM records r
        {join_event}
        WHERE map_id = ? AND player_id = ?
        {and_event}
        ORDER BY time {order} LIMIT 1",
        join_event = join_event,
        and_event = and_event,
        order = if reversed { "DESC" } else { "ASC" },
    );

    // We retrieve the optional old record to compare with the new one
    let mut query = sqlx::query_as::<_, Record>(&query)
        .bind(map_id)
        .bind(player_id);

    if let Some((event, edition)) = event {
        query = query.bind(event.id).bind(edition.id);
    }

    let old_record = query.fetch_optional(&db.mysql_pool).await?;

    let (old, new, has_improved) = if let Some(Record { time: old, .. }) = old_record {
        let improved = if reversed {
            body.time > old
        } else {
            body.time < old
        };

        (old, body.time, improved)
    } else {
        (body.time, body.time, true)
    };

    // We insert the record (whether it is the new personal best or not)
    let record_id = insert_record(
        db,
        &mut redis_conn,
        player_id,
        map_id,
        &body,
        &map_key,
        reversed,
        event,
    )
    .await?;

    // TODO: Remove this after having added event mode into the TP
    let original_uid = body.map_uid.replace("_benchmark", "");
    if original_uid != body.map_uid {
        let Map {
            id: map_id,
            cps_number: original_cps_number,
            reversed: original_reversed,
            ..
        } = must::have_map(db, &original_uid).await?;

        if cps_number == original_cps_number && reversed == original_reversed.unwrap_or(false) {
            let map_key = format_map_key(map_id, None);
            insert_record(
                db,
                &mut redis_conn,
                player_id,
                map_id,
                &HasFinishedBody {
                    time: body.time,
                    respawn_count: body.respawn_count,
                    map_uid: original_uid,
                    flags: body.flags,
                    cps: body.cps,
                },
                &map_key,
                reversed,
                None,
            )
            .await?;
        } else {
            return Err(RecordsError::MapNotFound(original_uid));
        }
    }

    let current_rank = get_rank_or_full_update(
        db,
        &mut redis_conn,
        &map_key,
        map_id,
        if reversed { old.max(new) } else { old.min(new) },
        reversed,
        event,
    )
    .await?;

    Ok(FinishedOutput {
        record_id,
        res: HasFinishedResponse {
            has_improved,
            login,
            old,
            new,
            current_rank,
            reversed,
        },
    })
}
