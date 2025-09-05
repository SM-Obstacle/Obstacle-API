use crate::{RecordsResult, RecordsResultExt, internal};
use actix_web::web::Query;
use entity::{event_edition_records, maps, records};
use records_lib::leaderboard::{self, Row};
use records_lib::opt_event::OptEvent;
use records_lib::ranks::update_leaderboard;
use records_lib::{RedisConnection, ranks};
use records_lib::{player, transaction};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
    StreamTrait, TransactionTrait,
};

#[derive(serde::Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "playerId")]
    pub(crate) login: String,
    #[serde(alias = "mapId")]
    pub(crate) map_uid: String,
}

pub type OverviewReq = Query<OverviewQuery>;

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

    // -- Compute display ranges
    const TOTAL_ROWS: i32 = 15;
    const NO_RECORD_ROWS: i32 = TOTAL_ROWS - 1;

    let player_rank = match player {
        Some(ref p) => {
            let min_time = records::Entity::find()
                .filter(
                    records::Column::RecordPlayerId
                        .eq(p.id)
                        .and(records::Column::MapId.eq(map.id)),
                )
                .apply_if(event.event, |query, (ev, ed)| {
                    query.inner_join(event_edition_records::Entity).filter(
                        event_edition_records::Column::EventId
                            .eq(ev.id)
                            .and(event_edition_records::Column::EditionId.eq(ed.id)),
                    )
                })
                .select_only()
                .column_as(records::Column::Time.min(), "min_time")
                .into_tuple()
                .one(conn)
                .await
                .with_api_err()?
                .ok_or_else(|| internal!("Query should return at least one row"))?;

            match min_time {
                Some(time) => {
                    let rank = ranks::get_rank(conn, redis_conn, map.id, p.id, time, event)
                        .await
                        .with_api_err()?;
                    Some(rank)
                }
                None => None,
            }
        }
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
            const ROWS_MINUS_TOP3: i32 = TOTAL_ROWS - 3;
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
