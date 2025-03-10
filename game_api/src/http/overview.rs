use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use deadpool_redis::redis::AsyncCommands;
use records_lib::context::{
    HasMap, HasMapId, HasPersistentMode, HasPlayerLogin, ReadOnly, Transactional,
};
use records_lib::leaderboard::{self, Row};
use records_lib::ranks::update_leaderboard;
use records_lib::{
    DatabaseConnection, MySqlConnection, RedisConnection, player, redis_key::map_key, transaction,
};

#[derive(serde::Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "playerId")]
    pub(crate) login: String,
    #[serde(alias = "mapId")]
    pub(crate) map_uid: String,
}

pub type OverviewReq = Query<OverviewQuery>;

async fn extend_range<C>(
    conn: &mut DatabaseConnection<'_>,
    records: &mut Vec<Row>,
    (start, end): (i64, i64),
    ctx: C,
) -> RecordsResult<()>
where
    C: HasMapId + Transactional,
{
    records.extend(
        leaderboard::leaderboard_txn(conn, ctx, Some(start), Some(end - 1))
            .await
            .with_api_err()?,
    );
    Ok(())
}

#[derive(serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct ResponseBody {
    pub response: Vec<Row>,
}

async fn build_records_array<C>(
    mysql_conn: MySqlConnection<'_>,
    redis_conn: &mut RedisConnection,
    ctx: C,
) -> RecordsResult<Vec<Row>>
where
    C: HasPlayerLogin + HasMap + Transactional<Mode = ReadOnly>,
{
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn,
    };

    let player = player::get_player_from_login(conn.mysql_conn, ctx.get_player_login())
        .await
        .with_api_err()?;

    // Update redis if needed
    let key = map_key(ctx.get_map().id, ctx.get_opt_event_edition());
    let count = update_leaderboard(&mut conn, &ctx).await?;

    let mut ranked_records = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: i64 = 15;
    const NO_RECORD_ROWS: i64 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = match player {
        Some(ref player) => conn
            .redis_conn
            .zrank(&key, player.id)
            .await
            .with_api_err()?,
        None => None,
    };

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            extend_range(&mut conn, &mut ranked_records, (0, TOTAL_ROWS), &ctx).await?;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            extend_range(&mut conn, &mut ranked_records, (0, 3), &ctx).await?;

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
            extend_range(&mut conn, &mut ranked_records, range, &ctx).await?;
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            extend_range(
                &mut conn,
                &mut ranked_records,
                (0, NO_RECORD_ROWS - 3),
                &ctx,
            )
            .await?;

            // last 3
            extend_range(&mut conn, &mut ranked_records, (count - 3, count), &ctx).await?;
        }
        // There is enough records to display them all
        else {
            extend_range(&mut conn, &mut ranked_records, (0, NO_RECORD_ROWS), &ctx).await?;
        }
    }

    Ok(ranked_records)
}

pub async fn overview<C>(conn: DatabaseConnection<'_>, ctx: C) -> RecordsResult<ResponseBody>
where
    C: HasPlayerLogin + HasMap + HasPersistentMode,
{
    let ranked_records =
        transaction::within(conn.mysql_conn, ctx, ReadOnly, async |mysql_conn, ctx| {
            build_records_array(mysql_conn, conn.redis_conn, ctx).await
        })
        .await?;

    Ok(ResponseBody {
        response: ranked_records,
    })
}
