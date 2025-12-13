use crate::{ApiErrorKind, RecordsResult, RecordsResultExt};
use actix_web::web::Json;
use deadpool_redis::redis;
use entity::{checkpoint_times, event_edition_records, maps, records, types};
use records_lib::{
    NullableInteger, RedisPool, opt_event::OptEvent, ranks, redis_key::map_key, sync,
};
use sea_orm::{
    ActiveValue::Set, ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter as _, QueryOrder,
    QuerySelect, QueryTrait, TransactionTrait,
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
    event: OptEvent<'a>,
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
        event,
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

    let record_id = records::Entity::insert(new_record)
        .exec(conn)
        .await?
        .last_insert_id;

    checkpoint_times::Entity::insert_many(body.cps.iter().enumerate().map(|(idx, time)| {
        checkpoint_times::ActiveModel {
            cp_num: Set(idx as _),
            map_id: Set(map_id),
            record_id: Set(record_id),
            time: Set(*time),
        }
    }))
    .exec(conn)
    .await?;

    if let Some((ev, ed)) = event.get() {
        let new_event_record = event_edition_records::ActiveModel {
            record_id: Set(record_id),
            event_id: Set(ev.id),
            edition_id: Set(ed.id),
        };
        event_edition_records::Entity::insert(new_event_record)
            .exec(conn)
            .await?;
    }

    Ok(record_id)
}

#[derive(Clone, Copy)]
pub struct ExpandedInsertRecordParams<'a> {
    pub body: &'a InsertRecordParams,
    pub at: chrono::NaiveDateTime,
    pub event: OptEvent<'a>,
    pub mode_version: Option<types::ModeVersion>,
}

pub(super) async fn insert_record<C>(
    conn: &C,
    params: ExpandedInsertRecordParams<'_>,
    map_id: u32,
    player_id: u32,
    event_record_id: Option<u32>,
) -> RecordsResult<u32>
where
    C: ConnectionTrait,
{
    let record_id = send_query(
        conn,
        player_id,
        map_id,
        SendQueryParam {
            body: params.body,
            event_record_id,
            at: params.at,
            mode_version: params.mode_version,
            event: params.event,
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

async fn lock_map_records<C: ConnectionTrait>(conn: &C, map_id: u32) -> RecordsResult<()> {
    // TODO(#105): once sea-query supports "LOCK IN SHARE MODE" syntax for MySQL/MariaDB,
    // use a shared lock instead of an exclusive one.
    // See https://github.com/SeaQL/sea-query/pull/980
    let _ = records::Entity::find()
        .select_only()
        .expr(1)
        .filter(records::Column::MapId.eq(map_id))
        .lock_exclusive()
        .into_tuple::<(i32,)>()
        .all(conn)
        .await
        .with_api_err()?;
    Ok(())
}

struct RecordSaveOutput {
    old_record: Option<records::Model>,
    record_id: u32,
}

pub async fn finished<C>(
    conn: &C,
    redis_pool: &RedisPool,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &maps::Model,
) -> RecordsResult<FinishedOutput>
where
    C: ConnectionTrait + TransactionTrait,
{
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

    let result = sync::transaction(conn, async |txn| {
        // Lock the rows related to the map
        lock_map_records(txn, map.id).await?;

        let old_record = get_old_record(txn, player_id, map.id, params.event).await?;
        let new_record_id = insert_record(txn, params, map.id, player_id, None).await?;

        RecordsResult::Ok(RecordSaveOutput {
            old_record,
            record_id: new_record_id,
        })
    })
    .await?;

    let (old, new, has_improved, old_rank) = match result.old_record {
        Some(records::Model { time: old, .. }) => (
            old,
            params.body.time,
            params.body.time < old,
            Some(ranks::get_rank(redis_pool, map.id, player_id, old, params.event).await?),
        ),
        None => (params.body.time, params.body.time, true, None),
    };

    let current_rank = if has_improved {
        // We update the score in Redis then get the new rank.
        //
        // N.B.: we must update the time in Redis **after** the SQL transaction is committed, so
        // that any other operation acting on the same leaderboard doesn't update the ZSET with an
        // outdated version.

        let mut pipe = redis::pipe();
        pipe.atomic();

        let map_key = map_key(map.id, params.event);
        pipe.zadd(&map_key, player_id, new)
            .ignore()
            .zcount(&map_key, "-inf", new - 1);

        let mut redis_conn = redis_pool.get().await.with_api_err()?;
        let (count,): (i32,) = pipe.query_async(&mut redis_conn).await.with_api_err()?;
        count + 1
    } else {
        ranks::get_rank(redis_pool, map.id, player_id, old, params.event)
            .await
            .with_api_err()?
    };

    Ok(FinishedOutput {
        record_id: result.record_id,
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
