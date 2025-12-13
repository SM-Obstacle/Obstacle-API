use crate::{RecordsResult, RecordsResultExt};
use actix_web::web;
use entity::{event_edition_records, maps, records};
use records_lib::leaderboard::{self, Row};
use records_lib::opt_event::OptEvent;
use records_lib::ranks::update_leaderboard;
use records_lib::{Database, RedisPool, ranks};
use records_lib::{player, sync};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
    StreamTrait,
};

// -- Compute display ranges
const TOTAL_ROWS: i32 = 15;
const NO_RECORD_ROWS: i32 = TOTAL_ROWS - 1;
const ROWS_MINUS_TOP3: i32 = TOTAL_ROWS - 3;

#[derive(serde::Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "playerId")]
    pub(crate) login: String,
    #[serde(alias = "mapId")]
    pub(crate) map_uid: String,
}

pub type OverviewReq = web::Query<OverviewQuery>;

async fn extend_range<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    records: &mut Vec<Row>,
    (start, end): (i32, i32),
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    leaderboard::leaderboard_into(
        conn,
        redis_pool,
        map_id,
        Some(start),
        Some(end - 1),
        records,
        event,
    )
    .await
    .with_api_err()?;
    Ok(())
}

#[derive(serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct ResponseBody {
    pub response: Vec<Row>,
}

async fn build_records_array<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    player_rank: Option<i32>,
    records_count: i32,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<Row>> {
    let mut ranked_records = Vec::new();

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            extend_range(
                conn,
                redis_pool,
                &mut ranked_records,
                (0, TOTAL_ROWS),
                map_id,
                event,
            )
            .await?;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            extend_range(conn, redis_pool, &mut ranked_records, (0, 3), map_id, event).await?;

            // the rest is centered around the player
            let range = {
                let start = player_rank - ROWS_MINUS_TOP3 / 2;
                let end = player_rank + ROWS_MINUS_TOP3 / 2;
                if end >= records_count {
                    (start - (end - records_count), records_count)
                } else {
                    (start, end)
                }
            };
            extend_range(conn, redis_pool, &mut ranked_records, range, map_id, event).await?;
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if records_count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            extend_range(
                conn,
                redis_pool,
                &mut ranked_records,
                (0, NO_RECORD_ROWS - 3),
                map_id,
                event,
            )
            .await?;

            // last 3
            extend_range(
                conn,
                redis_pool,
                &mut ranked_records,
                (records_count - 3, records_count),
                map_id,
                event,
            )
            .await?;
        }
        // There is enough records to display them all
        else {
            extend_range(
                conn,
                redis_pool,
                &mut ranked_records,
                (0, NO_RECORD_ROWS),
                map_id,
                event,
            )
            .await?;
        }
    }

    Ok(ranked_records)
}

async fn get_rank<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    map: &maps::Model,
    event: OptEvent<'_>,
    p: &entity::players::Model,
) -> Result<Option<i32>, crate::ApiErrorKind> {
    let min_time = records::Entity::find()
        .filter(
            records::Column::RecordPlayerId
                .eq(p.id)
                .and(records::Column::MapId.eq(map.id)),
        )
        .select_only()
        .expr(records::Column::Time.min())
        .apply_if(event.get(), |query, (ev, ed)| {
            query.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
        .into_tuple::<Option<i32>>()
        .one(conn)
        .await
        .with_api_err()?
        .flatten();

    match min_time {
        Some(time) => {
            let rank = ranks::get_rank(redis_pool, map.id, p.id, time, event)
                .await
                .with_api_err()?;
            Ok(Some(rank))
        }
        None => Ok(None),
    }
}

pub async fn overview(
    db: Database,
    player_login: &str,
    map: &maps::Model,
    event: OptEvent<'_>,
) -> RecordsResult<ResponseBody> {
    let player = player::get_player_from_login(&db.sql_conn, player_login)
        .await
        .with_api_err()?;

    // Update redis if needed
    let count = update_leaderboard(&db.sql_conn, &db.redis_pool, map.id, event).await? as _;

    let player_rank = match player {
        Some(ref p) => get_rank(&db.sql_conn, &db.redis_pool, map, event, p).await?,
        None => None,
    };

    let ranked_records = sync::transaction_with_config(
        &db.sql_conn,
        Some(sea_orm::IsolationLevel::RepeatableRead),
        Some(sea_orm::AccessMode::ReadOnly),
        async |txn| {
            build_records_array(txn, &db.redis_pool, player_rank, count, map.id, event).await
        },
    )
    .await?;

    Ok(ResponseBody {
        response: ranked_records,
    })
}
