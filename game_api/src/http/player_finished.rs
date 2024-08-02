use actix_web::web::Json;
use deadpool_redis::redis::AsyncCommands;
use futures::TryStreamExt;
use records_lib::{
    event::OptEvent,
    models,
    redis_key::map_key,
    update_ranks::{get_rank, update_leaderboard},
    DatabaseConnection,
};
use serde::{Deserialize, Serialize};
use sqlx::Connection;

use crate::{RecordsErrorKind, RecordsResult, RecordsResultExt};

use super::{event, map::MapParam};

#[derive(Deserialize, Debug, Clone)]
pub struct InsertRecordParams {
    pub time: i32,
    pub respawn_count: i32,
    pub flags: Option<u32>,
    pub cps: Vec<i32>,
}

pub struct FinishedParams<'a> {
    pub rest: InsertRecordParams,
    pub map: MapParam<'a>,
}

#[derive(Deserialize, Debug)]
pub struct HasFinishedBody {
    pub map_uid: String,
    #[serde(flatten)]
    pub rest: InsertRecordParams,
}

impl HasFinishedBody {
    #[inline]
    pub fn into_params(self, map: Option<&models::Map>) -> FinishedParams<'_> {
        FinishedParams {
            rest: self.rest,
            map: MapParam::from_map(map, self.map_uid),
        }
    }
}

pub type PlayerFinishedBody = Json<HasFinishedBody>;

#[derive(Deserialize, Serialize)]
pub struct HasFinishedResponse {
    has_improved: bool,
    login: String,
    old: i32,
    new: i32,
    current_rank: i32,
}

async fn send_query(
    db: &mut sqlx::MySqlConnection,
    map_id: u32,
    player_id: u32,
    body: InsertRecordParams,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
) -> records_lib::error::RecordsResult<u32> {
    let record_id: u32 = sqlx::query_scalar(
        "INSERT INTO records (record_player_id, map_id, time, respawn_count, record_date, flags, event_record_id)
                    VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING record_id",
    )
    .bind(player_id)
    .bind(map_id)
    .bind(body.time)
    .bind(body.respawn_count)
    .bind(at)
    .bind(body.flags)
    .bind(event_record_id)
    .fetch_one(&mut *db)
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
    .execute(db)
    .await?;

    Ok(record_id)
}

pub(super) async fn insert_record(
    db: &mut DatabaseConnection,
    map_id: u32,
    player_id: u32,
    body: InsertRecordParams,
    event: OptEvent<'_, '_>,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
) -> RecordsResult<u32> {
    let key = map_key(map_id, event);
    let added: Option<i64> = db.redis_conn.zadd(key, player_id, body.time).await.ok();
    if added.is_none() {
        let _count = update_leaderboard(db, map_id, event).await?;
    }

    let record_id = db
        .mysql_conn
        .transaction(|txn| {
            Box::pin(send_query(
                txn,
                map_id,
                player_id,
                body,
                event_record_id,
                at,
            ))
        })
        .await?;

    Ok(record_id)
}

pub struct FinishedOutput {
    pub record_id: u32,
    pub player_id: u32,
    pub res: HasFinishedResponse,
}

pub async fn finished(
    login: String,
    db: &mut DatabaseConnection,
    params: FinishedParams<'_>,
    event: OptEvent<'_, '_>,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
) -> RecordsResult<FinishedOutput> {
    // First, we retrieve all what we need to save the record
    let player_id = records_lib::must::have_player(&mut db.mysql_conn, &login)
        .await?
        .id;
    let map @ models::Map {
        id: map_id,
        cps_number,
        ..
    } = match params.map {
        MapParam::AlreadyQueried(map) => map,
        MapParam::Uid(uid) => &records_lib::must::have_map(&mut db.mysql_conn, &uid).await?,
    };

    let (join_event, and_event) = event.get_join();

    // We check that the cps times are coherent to the final time
    if matches!(cps_number, Some(num) if num + 1 != params.rest.cps.len() as u32)
        || params.rest.cps.iter().sum::<i32>() != params.rest.time
    {
        return Err(RecordsErrorKind::InvalidTimes);
    }

    let query = format!(
        "SELECT r.* FROM records r
        {join_event}
        WHERE map_id = ? AND record_player_id = ?
        {and_event}
        ORDER BY time LIMIT 1",
        join_event = join_event,
        and_event = and_event,
    );

    // We retrieve the optional old record to compare with the new one
    let mut query = sqlx::query_as::<_, models::Record>(&query)
        .bind(map_id)
        .bind(player_id);

    if let Some((event, edition)) = event.0 {
        query = query.bind(event.id).bind(edition.id);
    }

    let old_record = query
        .fetch_optional(&mut *db.mysql_conn)
        .await
        .with_api_err()?;

    let (old, new, has_improved) = if let Some(models::Record { time: old, .. }) = old_record {
        let improved = params.rest.time < old;

        (old, params.rest.time, improved)
    } else {
        (params.rest.time, params.rest.time, true)
    };

    // We insert the record (whether it is the new personal best or not)
    let record_id = insert_record(
        db,
        map.id,
        player_id,
        params.rest.clone(),
        event,
        event_record_id,
        at,
    )
    .await?;

    let current_rank = get_rank(db, map.id, old.min(new), event).await?;

    // If the record isn't in an event context, save the record to the events that have the map
    // and allow records saving without an event context.
    // If `event_record_id` is filled, then this record is simply a clone of the event record.
    if event.0.is_none() && event_record_id.is_none() {
        let editions = records_lib::event::get_editions_which_contain(&mut db.mysql_conn, *map_id)
            .try_collect::<Vec<_>>()
            .await
            .with_api_err()?;

        for (event_id, edition_id, original_map_id) in editions {
            // If `original_map_id` is filled, the event map has an original map. But we can't
            // make a record for an event on the original map, without passing by the event context
            // (`/event/:event/:edition/player/finished`). Thus, the `on_original` attribute is
            // true if the map doesn't have an original map, so that it can be retrieved from the
            // `global_records` view.
            let on_original = original_map_id.is_none();

            event::insert_event_record(
                &mut db.mysql_conn,
                record_id,
                event_id,
                edition_id,
                on_original,
            )
            .await?;

            let Some(original_map_id) = original_map_id else {
                continue;
            };

            insert_record(
                db,
                original_map_id,
                player_id,
                params.rest.clone(),
                Default::default(),
                Some(record_id),
                at,
            )
            .await?;
        }
    }

    Ok(FinishedOutput {
        record_id,
        player_id,
        res: HasFinishedResponse {
            has_improved,
            login,
            old,
            new,
            current_rank,
        },
    })
}
