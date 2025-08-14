use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use deadpool_redis::redis::AsyncCommands;
use entity::maps;
use records_lib::RedisConnection;
use records_lib::leaderboard::{self, Row};
use records_lib::opt_event::OptEvent;
use records_lib::ranks::update_leaderboard;
use records_lib::{player, redis_key::map_key, transaction};
use sea_orm::{ConnectionTrait, StreamTrait, TransactionTrait};

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
    (start, end): (i64, i64),
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
    let key = map_key(map.id, event);
    let count = update_leaderboard(conn, redis_conn, map.id, event).await?;

    let mut ranked_records = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: i64 = 15;
    const NO_RECORD_ROWS: i64 = TOTAL_ROWS - 1;

    // N.B. Though Redis ordering for players having the same time on a map is based on their ID, rather
    // than their record date, this doesn't really matter. Indeed, this small difference could be noticed
    // when:
    // - On the same map, two players made the same time
    // - Both players have the rank 15
    // - The player who made the record earlier has an ID that is greater than the one who made the record later.
    // What the overview request should provide as the leaderboard for the player who made the record earlier:
    // - The top 15, with the player being 15th.
    // What it actually provides:
    // - The top 3, then the leaderboard centered around the player, still being 15th (using the ranks
    //   module), but with the other player who made the record later being ahead of them.
    let player_rank: Option<i64> = match player {
        Some(ref player) => redis_conn.zrank(&key, player.id).await.with_api_err()?,
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
            let row_minus_top3 = TOTAL_ROWS - 3;
            let range = {
                let start = player_rank - row_minus_top3 / 2;
                let end = player_rank + row_minus_top3 / 2;
                if end >= count as _ {
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
