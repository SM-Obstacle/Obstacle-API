use crate::{RecordsResult, RecordsResultExt};
use actix_web::web;
use entity::{global_event_records, global_records, maps, records};
use records_lib::leaderboard::{self, Row};
use records_lib::opt_event::OptEvent;
use records_lib::ranks::update_leaderboard;
use records_lib::{RedisConnection, ranks};
use records_lib::{player, transaction};
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Query;
use sea_orm::{ConnectionTrait, StreamTrait, TransactionTrait};

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
    redis_conn: &mut RedisConnection,
    records: &mut Vec<Row>,
    (start, end): (i32, i32),
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    leaderboard::leaderboard_into(
        conn,
        redis_conn,
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
    redis_conn: &mut RedisConnection,
    player_login: &str,
    map: &maps::Model,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<Row>> {
    let player = player::get_player_from_login(conn, player_login)
        .await
        .with_api_err()?;

    // Update redis if needed
    let count = update_leaderboard(conn, redis_conn, map.id, event).await? as _;

    let mut ranked_records = vec![];

    let player_rank = match player {
        Some(ref p) => get_rank(conn, redis_conn, map, event, p).await?,
        None => None,
    };

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            extend_range(
                conn,
                redis_conn,
                &mut ranked_records,
                (0, TOTAL_ROWS),
                map.id,
                event,
            )
            .await?;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            extend_range(conn, redis_conn, &mut ranked_records, (0, 3), map.id, event).await?;

            // the rest is centered around the player
            let range = {
                let start = player_rank - ROWS_MINUS_TOP3 / 2;
                let end = player_rank + ROWS_MINUS_TOP3 / 2;
                if end >= count {
                    (start - (end - count), count)
                } else {
                    (start, end)
                }
            };
            extend_range(conn, redis_conn, &mut ranked_records, range, map.id, event).await?;
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            extend_range(
                conn,
                redis_conn,
                &mut ranked_records,
                (0, NO_RECORD_ROWS - 3),
                map.id,
                event,
            )
            .await?;

            // last 3
            extend_range(
                conn,
                redis_conn,
                &mut ranked_records,
                (count - 3, count),
                map.id,
                event,
            )
            .await?;
        }
        // There is enough records to display them all
        else {
            extend_range(
                conn,
                redis_conn,
                &mut ranked_records,
                (0, NO_RECORD_ROWS),
                map.id,
                event,
            )
            .await?;
        }
    }

    Ok(ranked_records)
}

async fn get_rank<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut deadpool_redis::Connection,
    map: &maps::Model,
    event: OptEvent<'_>,
    p: &entity::players::Model,
) -> Result<Option<i32>, crate::RecordsErrorKind> {
    let mut query = Query::select();

    query
        .and_where(
            Expr::col(("r", records::Column::RecordPlayerId))
                .eq(p.id)
                .and(Expr::col(("r", records::Column::MapId)).eq(map.id)),
        )
        .expr(Expr::col(("r", records::Column::Time)));

    match event.event {
        Some((ev, ed)) => {
            query.from_as(global_event_records::Entity, "r").and_where(
                Expr::col(("r", global_event_records::Column::EventId))
                    .eq(ev.id)
                    .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
            );
        }
        None => {
            query.from_as(global_records::Entity, "r");
        }
    }

    let stmt = conn.get_database_backend().build(&query);

    let min_time = conn
        .query_one(stmt)
        .await
        .and_then(|result_opt| {
            result_opt
                .map(|result| result.try_get_by_index::<i32>(0))
                .transpose()
        })
        .with_api_err()?;

    match min_time {
        Some(time) => {
            let rank = ranks::get_rank(conn, redis_conn, map.id, p.id, time, event)
                .await
                .with_api_err()?;
            Ok(Some(rank))
        }
        None => Ok(None),
    }
}

pub async fn overview<C: TransactionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    player_login: &str,
    map: &maps::Model,
    event: OptEvent<'_>,
) -> RecordsResult<ResponseBody> {
    let ranked_records = transaction::within(conn, async |txn| {
        build_records_array(txn, redis_conn, player_login, map, event).await
    })
    .await?;

    Ok(ResponseBody {
        response: ranked_records,
    })
}
