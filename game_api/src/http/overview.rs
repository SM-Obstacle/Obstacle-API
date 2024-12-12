use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use deadpool_redis::redis::AsyncCommands;
use records_lib::context::{Ctx, HasMap, HasMapId, HasPlayer, HasPlayerLogin};
use records_lib::{
    must,
    ranks::{get_rank, update_leaderboard},
    redis_key::map_key,
    DatabaseConnection,
};
use records_lib::{table_lock, RedisConnection};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "playerId")]
    pub(crate) login: String,
    #[serde(alias = "mapId")]
    pub(crate) map_uid: String,
}

pub type OverviewReq = Query<OverviewQuery>;

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct RecordQueryRow {
    pub login: String,
    pub player_id: u32,
    pub nickname: String,
    pub time: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename = "records")]
pub struct RankedRecord {
    pub rank: u32,
    pub login: String,
    pub nickname: String,
    pub time: i32,
}

async fn extend_range<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    records: &mut Vec<RankedRecord>,
    (start, end): (u32, u32),
    ctx: C,
) -> RecordsResult<()> {
    // transforms exclusive to inclusive range
    let end = end - 1;
    let player_ids: Vec<i32> = conn
        .redis_conn
        .zrange(
            map_key(ctx.get_map_id(), ctx.get_opt_event_edition()),
            start as isize,
            end as isize,
        )
        .await
        .with_api_err()?;

    if player_ids.is_empty() {
        // There is no player in this range
        return Ok(());
    }

    let builder = ctx.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new(
        "SELECT CAST(0 AS UNSIGNED) AS rank,
            p.id AS player_id,
            p.login AS login,
            p.name AS nickname,
            min(time) as time,
            map_id
        FROM records r ",
    );
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(
            "INNER JOIN players p ON r.record_player_id = p.id \
            WHERE map_id = ",
        )
        .push_bind(ctx.get_map_id())
        .push(" AND record_player_id IN (");
    let mut sep = query.separated(", ");
    for id in player_ids {
        sep.push_bind(id);
    }
    query.push(") ");
    let result = builder
        .push_event_filter(&mut query, "eer")
        .push(" GROUP BY record_player_id ORDER BY time, record_date ASC")
        .build_query_as::<RecordQueryRow>()
        .fetch_all(&mut **conn.mysql_conn)
        .await
        .with_api_err()?;

    records.reserve(result.len());

    for r in result {
        records.push(RankedRecord {
            rank: get_rank(conn, Ctx::with_player_id(&ctx, r.player_id), r.time).await? as _,
            login: r.login,
            nickname: r.nickname,
            time: r.time,
        })
    }

    Ok(())
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Deserialize))]
pub struct ResponseBody {
    pub response: Vec<RankedRecord>,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error(transparent)]
    Lib(#[from] records_lib::error::RecordsError),
    #[error(transparent)]
    Api(#[from] crate::RecordsErrorKind),
}

impl From<sqlx::Error> for Error {
    fn from(value: sqlx::Error) -> Self {
        Self::Lib(value.into())
    }
}

struct BuildRecordsArrayParam<'a, C> {
    redis_conn: &'a mut RedisConnection,
    ctx: C,
}

async fn build_records_array<C>(
    mysql_conn: &mut sqlx::pool::PoolConnection<sqlx::MySql>,
    param: BuildRecordsArrayParam<'_, C>,
) -> Result<Vec<RankedRecord>, Error>
where
    C: HasPlayer + HasMap,
{
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn: param.redis_conn,
    };

    // Update redis if needed
    let key = map_key(param.ctx.get_map().id, param.ctx.get_opt_event_edition());
    let count = update_leaderboard(&mut conn, &param.ctx).await? as u32;

    let mut ranked_records = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: u32 = 15;
    const NO_RECORD_ROWS: u32 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = conn
        .redis_conn
        .zrank(&key, param.ctx.get_player_id())
        .await
        .with_api_err()?;
    let player_rank = player_rank.map(|r| r as u64 as u32);

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            extend_range(&mut conn, &mut ranked_records, (0, TOTAL_ROWS), &param.ctx).await?;
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            extend_range(&mut conn, &mut ranked_records, (0, 3), &param.ctx).await?;

            // the rest is centered around the player
            let row_minus_top3 = TOTAL_ROWS - 3;
            let range = {
                let start = player_rank - row_minus_top3 / 2;
                let end = player_rank + row_minus_top3 / 2;
                if end >= count {
                    (start - (end - count), count)
                } else {
                    (start, end)
                }
            };
            extend_range(&mut conn, &mut ranked_records, range, &param.ctx).await?;
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
                &param.ctx,
            )
            .await?;

            // last 3
            extend_range(
                &mut conn,
                &mut ranked_records,
                (count - 3, count),
                &param.ctx,
            )
            .await?;
        }
        // There is enough records to display them all
        else {
            extend_range(
                &mut conn,
                &mut ranked_records,
                (0, NO_RECORD_ROWS),
                &param.ctx,
            )
            .await?;
        }
    }

    Ok(ranked_records)
}

pub async fn overview<C>(conn: DatabaseConnection<'_>, ctx: C) -> RecordsResult<Vec<RankedRecord>>
where
    C: HasPlayerLogin + HasMap,
{
    let player = must::have_player(conn.mysql_conn, &ctx)
        .await
        .with_api_err()?;
    let ctx = ctx.with_player(&player);

    let ranked_records = match table_lock::locked_records(
        conn.mysql_conn,
        BuildRecordsArrayParam {
            redis_conn: conn.redis_conn,
            ctx,
        },
        build_records_array,
    )
    .await
    {
        Ok(records) => records,
        Err(Error::Api(e)) => return Err(e),
        Err(Error::Lib(e)) => return Err(e).with_api_err(),
    };

    Ok(ranked_records)
}
