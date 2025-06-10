use crate::{RecordsErrorKind, RecordsResult, RecordsResultExt};
use actix_web::web::Json;
use records_lib::{
    MySqlConnection, NullableInteger, TxnDatabaseConnection, models, opt_event::OptEvent, ranks,
    transaction::CanWrite,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct InsertRecordParams {
    pub time: i32,
    pub respawn_count: i32,
    pub flags: Option<u32>,
    pub cps: Vec<i32>,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(test, derive(Serialize))]
pub struct HasFinishedBody {
    pub map_uid: String,
    #[serde(flatten)]
    pub rest: InsertRecordParams,
}

pub type PlayerFinishedBody = Json<HasFinishedBody>;

#[derive(Serialize)]
pub struct HasFinishedResponse {
    has_improved: bool,
    old: i32,
    new: i32,
    current_rank: i32,
    old_rank: NullableInteger,
}

struct SendQueryParam<'a> {
    body: &'a InsertRecordParams,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
    mode_version: Option<records_lib::ModeVersion>,
}

async fn send_query(
    db: MySqlConnection<'_>,
    player_id: u32,
    map_id: u32,
    SendQueryParam {
        body,
        event_record_id,
        at,
        mode_version,
    }: SendQueryParam<'_>,
) -> sqlx::Result<u32> {
    let record_id: u32 = sqlx::query_scalar(
        "INSERT INTO records (record_player_id, map_id, time, respawn_count, record_date, flags, event_record_id, modeversion)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING record_id",
    )
    .bind(player_id)
    .bind(map_id)
    .bind(body.time)
    .bind(body.respawn_count)
    .bind(at)
    .bind(body.flags)
    .bind(event_record_id)
    .bind(mode_version)
    .fetch_one(&mut **db)
    .await?;

    let mut query =
        sqlx::QueryBuilder::new("INSERT INTO checkpoint_times (cp_num, map_id, record_id, time) ");
    query
        .push_values(body.cps.iter().enumerate(), |mut b, (i, cptime)| {
            b.push_bind(i as u32)
                .push_bind(map_id)
                .push_bind(record_id)
                .push_bind(cptime);
        })
        .build()
        .execute(&mut **db)
        .await?;

    Ok(record_id)
}

#[derive(Clone, Copy)]
pub struct ExpandedInsertRecordParams<'a> {
    pub body: &'a InsertRecordParams,
    pub at: chrono::NaiveDateTime,
    pub event: OptEvent<'a>,
    pub mode_version: Option<records_lib::ModeVersion>,
}

pub(super) async fn insert_record<M: CanWrite>(
    conn: &mut TxnDatabaseConnection<'_, M>,
    params: ExpandedInsertRecordParams<'_>,
    map_id: u32,
    player_id: u32,
    event_record_id: Option<u32>,
    update_redis_lb: bool,
) -> RecordsResult<u32> {
    ranks::update_leaderboard(conn, map_id, params.event).await?;

    if update_redis_lb {
        ranks::update_rank(
            conn.conn.redis_conn,
            map_id,
            player_id,
            params.body.time,
            params.event,
        )
        .await?;
    }

    // FIXME: find a way to retry deadlock errors **without loops**
    let record_id = send_query(
        conn.conn.mysql_conn,
        player_id,
        map_id,
        SendQueryParam {
            body: params.body,
            event_record_id,
            at: params.at,
            mode_version: params.mode_version,
        },
    )
    .await
    .with_api_err()?;

    Ok(record_id)
}

pub struct FinishedOutput {
    pub record_id: u32,
    pub player_id: u32,
    pub res: HasFinishedResponse,
}

async fn get_old_record(
    db: &mut sqlx::MySqlConnection,
    player_id: u32,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<Option<models::Record>> {
    let builder = event.sql_frag_builder();
    let mut query = sqlx::QueryBuilder::new("SELECT r.* FROM records r ");
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(" where map_id = ")
        .push_bind(map_id)
        .push(" and record_player_id = ")
        .push_bind(player_id)
        .push(" ");
    builder
        .push_event_filter(&mut query, "eer")
        .push(" order by time limit 1")
        .build_query_as::<models::Record>()
        .fetch_optional(db)
        .await
        .with_api_err()
}

// conn, guard, params, at, mode_version, event

pub async fn finished<M: CanWrite>(
    conn: &mut TxnDatabaseConnection<'_, M>,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &models::Map,
) -> RecordsResult<FinishedOutput> {
    // First, we retrieve all what we need to save the record
    let player = records_lib::must::have_player(conn.conn.mysql_conn, player_login)
        .await
        .with_api_err()?;
    let player_id = player.id;

    // Return an error if the player was banned at the time.
    if let Some(ban) =
        super::player::get_ban_during(conn.conn.mysql_conn, player_id, params.at).await?
    {
        return Err(RecordsErrorKind::BannedPlayer(ban));
    }

    // We check that the cps times are coherent to the final time
    if matches!(map.cps_number, Some(num) if num + 1 != params.body.cps.len() as u32)
        || params.body.cps.iter().sum::<i32>() != params.body.time
    {
        return Err(RecordsErrorKind::InvalidTimes);
    }

    let old_record = get_old_record(conn.conn.mysql_conn, player_id, map.id, params.event).await?;

    let (old, new, has_improved, old_rank) =
        if let Some(models::Record { time: old, .. }) = old_record {
            (
                old,
                params.body.time,
                params.body.time < old,
                Some(ranks::get_rank(conn, map.id, player_id, old, params.event).await?),
            )
        } else {
            (params.body.time, params.body.time, true, None)
        };

    let event = params.event;

    // We insert the record
    let record_id = insert_record(conn, params, map.id, player_id, None, has_improved).await?;

    let current_rank = ranks::get_rank(
        conn,
        map.id,
        player_id,
        if has_improved { new } else { old },
        event,
    )
    .await?;

    Ok(FinishedOutput {
        record_id,
        player_id,
        res: HasFinishedResponse {
            has_improved,
            old,
            new,
            current_rank,
            old_rank: old_rank.into(),
        },
    })
}
