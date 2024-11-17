use crate::{RecordsErrorKind, RecordsResult, RecordsResultExt};
use actix_web::web::Json;
use futures::TryStreamExt;
use itertools::Itertools;
use records_lib::{event::OptEvent, models, player, ranks, DatabaseConnection, NullableInteger};
use serde::{Deserialize, Serialize};
use sqlx::Connection;

use super::{event, map::MapParam};

#[derive(Deserialize, Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
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
#[cfg_attr(test, derive(Serialize))]
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

#[derive(Serialize)]
pub struct HasFinishedResponse {
    has_improved: bool,
    login: String,
    old: i32,
    new: i32,
    current_rank: i32,
    old_rank: NullableInteger,
}

async fn send_query(
    db: &mut sqlx::MySqlConnection,
    map_id: u32,
    player_id: u32,
    body: InsertRecordParams,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
) -> sqlx::Result<u32> {
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

    let cps_times = body.cps.iter().enumerate().format_with(", ", |(i, t), f| {
        f(&format_args!("({i}, {map_id}, {record_id}, {t})"))
    });

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

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_record(
    db: &mut DatabaseConnection,
    map_id: u32,
    player_id: u32,
    body: InsertRecordParams,
    event: OptEvent<'_, '_>,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
    update_redis_lb: bool,
) -> RecordsResult<u32> {
    ranks::update_leaderboard(db, map_id, event).await?;

    if update_redis_lb {
        ranks::update_rank(&mut db.redis_conn, map_id, player_id, body.time, event).await?;
    }

    // FIXME: find a way to retry deadlock errors **without loops**
    let record_id = db
        .mysql_conn
        .transaction(|txn| {
            Box::pin(send_query(
                txn,
                map_id,
                player_id,
                body.clone(),
                event_record_id,
                at,
            ))
        })
        .await
        .with_api_err()?;

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

    let (old, new, has_improved, old_rank) =
        if let Some(models::Record { time: old, .. }) = old_record {
            (
                old,
                params.rest.time,
                params.rest.time < old,
                Some(ranks::get_rank(db, map.id, player_id, old, event).await?),
            )
        } else {
            (params.rest.time, params.rest.time, true, None)
        };

    // We insert the record
    let record_id = insert_record(
        db,
        map.id,
        player_id,
        params.rest.clone(),
        event,
        None,
        at,
        has_improved,
    )
    .await?;

    let current_rank = ranks::get_rank(
        db,
        map.id,
        player_id,
        if has_improved { new } else { old },
        event,
    )
    .await?;

    // If the record isn't in an event context, save the record to the events that have the map
    // and allow records saving without an event context.
    if event.0.is_none() {
        let editions = records_lib::event::get_editions_which_contain(&mut db.mysql_conn, *map_id)
            .try_collect::<Vec<_>>()
            .await
            .with_api_err()?;

        for (event_id, edition_id, original_map_id) in editions {
            event::insert_event_record(&mut db.mysql_conn, record_id, event_id, edition_id).await?;

            let Some(original_map_id) = original_map_id else {
                continue;
            };

            // Get previous the time of the player on the original map, to check if it would be a PB or not.
            let previous_time = player::get_time_on_map(
                &mut db.mysql_conn,
                player_id,
                original_map_id,
                Default::default(),
            )
            .await?;
            let is_pb =
                previous_time.is_none() || previous_time.is_some_and(|t| t > params.rest.time);

            insert_record(
                db,
                original_map_id,
                player_id,
                params.rest.clone(),
                Default::default(),
                Some(record_id),
                at,
                is_pb,
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
            old_rank: old_rank.map(From::from).into(),
        },
    })
}
