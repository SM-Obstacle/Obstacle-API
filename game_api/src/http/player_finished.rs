use crate::{RecordsErrorKind, RecordsResult, RecordsResultExt};
use actix_web::web::Json;
use records_lib::{
    context::{HasMap, HasMapId, HasPlayerId, HasPlayerLogin, ReadWrite, Transactional},
    models, ranks, DatabaseConnection, MySqlConnection, NullableInteger,
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
}

async fn send_query<C>(
    db: MySqlConnection<'_>,
    ctx: C,
    SendQueryParam {
        body,
        event_record_id,
        at,
    }: SendQueryParam<'_>,
) -> sqlx::Result<u32>
where
    C: HasPlayerId + HasMapId,
{
    let record_id: u32 = sqlx::query_scalar(
        "INSERT INTO records (record_player_id, map_id, time, respawn_count, record_date, flags, event_record_id)
                    VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING record_id",
    )
    .bind(ctx.get_player_id())
    .bind(ctx.get_map_id())
    .bind(body.time)
    .bind(body.respawn_count)
    .bind(at)
    .bind(body.flags)
    .bind(event_record_id)
    .fetch_one(&mut **db)
    .await?;

    let mut query =
        sqlx::QueryBuilder::new("INSERT INTO checkpoint_times (cp_num, map_id, record_id, time) ");
    query
        .push_values(body.cps.iter().enumerate(), |mut b, (i, cptime)| {
            b.push_bind(i as u32)
                .push_bind(ctx.get_map_id())
                .push_bind(record_id)
                .push_bind(cptime);
        })
        .build()
        .execute(&mut **db)
        .await?;

    Ok(record_id)
}

pub(super) async fn insert_record<C>(
    db: &mut DatabaseConnection<'_>,
    ctx: C,
    body: &InsertRecordParams,
    event_record_id: Option<u32>,
    at: chrono::NaiveDateTime,
    update_redis_lb: bool,
) -> RecordsResult<u32>
where
    C: HasMapId + HasPlayerId + Transactional<Mode = ReadWrite>,
{
    if update_redis_lb {
        ranks::update_rank(db.redis_conn, &ctx, body.time).await?;
    }

    // FIXME: find a way to retry deadlock errors **without loops**
    let record_id = send_query(
        db.mysql_conn,
        ctx,
        SendQueryParam {
            body,
            event_record_id,
            at,
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

async fn get_old_record<C>(
    db: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<Option<models::Record>>
where
    C: HasPlayerId + HasMapId,
{
    let builder = ctx.sql_frag_builder();
    let mut query = sqlx::QueryBuilder::new("SELECT r.* FROM records r ");
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" and record_player_id = ")
        .push_bind(ctx.get_player_id())
        .push(" ");
    builder
        .push_event_filter(&mut query, "eer")
        .push(" order by time limit 1")
        .build_query_as::<models::Record>()
        .fetch_optional(db)
        .await
        .with_api_err()
}

pub async fn finished<C>(
    db: &mut DatabaseConnection<'_>,
    ctx: C,
    params: InsertRecordParams,
    at: chrono::NaiveDateTime,
) -> RecordsResult<FinishedOutput>
where
    C: HasPlayerLogin + HasMap + Transactional<Mode = ReadWrite>,
{
    // First, we retrieve all what we need to save the record
    let player = records_lib::must::have_player(db.mysql_conn, &ctx)
        .await
        .with_api_err()?;
    let ctx = ctx.with_player(&player);
    let player_id = player.id;

    // We check that the cps times are coherent to the final time
    if matches!(ctx.get_map().cps_number, Some(num) if num + 1 != params.cps.len() as u32)
        || params.cps.iter().sum::<i32>() != params.time
    {
        return Err(RecordsErrorKind::InvalidTimes);
    }

    let old_record = get_old_record(db.mysql_conn, &ctx).await?;

    let (old, new, has_improved, old_rank) =
        if let Some(models::Record { time: old, .. }) = old_record {
            (
                old,
                params.time,
                params.time < old,
                Some(ranks::get_rank(db, &ctx, old).await?),
            )
        } else {
            (params.time, params.time, true, None)
        };

    // We insert the record
    let record_id = insert_record(db, &ctx, &params, None, at, has_improved).await?;

    let current_rank = ranks::get_rank(db, &ctx, if has_improved { new } else { old }).await?;

    Ok(FinishedOutput {
        record_id,
        player_id,
        res: HasFinishedResponse {
            has_improved,
            old,
            new,
            current_rank,
            old_rank: old_rank.map(From::from).into(),
        },
    })
}
