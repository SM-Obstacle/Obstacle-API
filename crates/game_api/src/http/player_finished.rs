use crate::{ApiErrorKind, RecordsResult, RecordsResultExt};
use actix_web::web::Json;
use entity::{checkpoint_times, event_edition_records, maps, records, types};
use records_lib::{NullableInteger, RedisConnection, opt_event::OptEvent, ranks};
use sea_orm::{
    ActiveValue::Set, ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter as _, QueryOrder,
    QuerySelect, QueryTrait, StreamTrait,
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
    mode_version: Option<types::ModeVersion>,
}

async fn send_query<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
    map_id: u32,
    SendQueryParam {
        body,
        event_record_id,
        at,
        mode_version,
    }: SendQueryParam<'_>,
) -> Result<u32, sea_orm::DbErr> {
    let new_record = records::ActiveModel {
        record_player_id: Set(player_id),
        map_id: Set(map_id),
        time: Set(body.time),
        respawn_count: Set(body.respawn_count),
        record_date: Set(at),
        flags: body.flags.map(Set).unwrap_or_default(),
        event_record_id: Set(event_record_id),
        modeversion: Set(mode_version),
        ..Default::default()
    };

    let record = records::Entity::insert(new_record)
        .exec_with_returning(conn)
        .await?;

    checkpoint_times::Entity::insert_many(body.cps.iter().enumerate().map(|(idx, time)| {
        checkpoint_times::ActiveModel {
            cp_num: Set(idx as _),
            map_id: Set(map_id),
            record_id: Set(record.record_id),
            time: Set(*time),
        }
    }))
    .exec(conn)
    .await?;

    Ok(record.record_id)
}

#[derive(Clone, Copy)]
pub struct ExpandedInsertRecordParams<'a> {
    pub body: &'a InsertRecordParams,
    pub at: chrono::NaiveDateTime,
    pub event: OptEvent<'a>,
    pub mode_version: Option<types::ModeVersion>,
}

pub(super) async fn insert_record<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    params: ExpandedInsertRecordParams<'_>,
    map_id: u32,
    player_id: u32,
    event_record_id: Option<u32>,
    update_redis_lb: bool,
) -> RecordsResult<u32> {
    ranks::update_leaderboard(conn, redis_conn, map_id, params.event).await?;

    if update_redis_lb {
        ranks::update_rank(
            redis_conn,
            map_id,
            player_id,
            params.body.time,
            params.event,
        )
        .await?;
    }

    // FIXME: find a way to retry deadlock errors **without loops**
    let record_id = send_query(
        conn,
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

async fn get_old_record<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<Option<records::Model>> {
    records::Entity::find()
        .filter(
            records::Column::MapId
                .eq(map_id)
                .and(records::Column::RecordPlayerId.eq(player_id)),
        )
        .order_by_asc(records::Column::Time)
        .limit(1)
        .apply_if(event.get(), |q, (ev, ed)| {
            q.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
        .one(conn)
        .await
        .with_api_err()
}

pub async fn finished<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &maps::Model,
) -> RecordsResult<FinishedOutput> {
    // First, we retrieve all what we need to save the record
    let player = records_lib::must::have_player(conn, player_login)
        .await
        .with_api_err()?;
    let player_id = player.id;

    // Return an error if the player was banned at the time.
    if let Some(ban) = super::player::get_ban_during(conn, player_id, params.at).await? {
        return Err(ApiErrorKind::BannedPlayer(ban));
    }

    // We check that the cps times are coherent to the final time
    if matches!(map.cps_number, Some(num) if num + 1 != params.body.cps.len() as u32)
        || params.body.cps.iter().sum::<i32>() != params.body.time
    {
        return Err(ApiErrorKind::InvalidTimes);
    }

    let old_record = get_old_record(conn, player_id, map.id, params.event).await?;

    let (old, new, has_improved, old_rank) = match old_record {
        Some(records::Model { time: old, .. }) => (
            old,
            params.body.time,
            params.body.time < old,
            Some(ranks::get_rank(conn, redis_conn, map.id, player_id, old, params.event).await?),
        ),
        None => (params.body.time, params.body.time, true, None),
    };

    let event = params.event;

    // We insert the record
    let record_id = insert_record(
        conn,
        redis_conn,
        params,
        map.id,
        player_id,
        None,
        has_improved,
    )
    .await?;

    let current_rank = ranks::get_rank(
        conn,
        redis_conn,
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
