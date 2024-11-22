use std::iter;

use crate::{utils, FitRequestId, RecordsResponse, RecordsResult, RecordsResultExt};
use actix_web::{web::Query, Responder};
use deadpool_redis::redis::AsyncCommands;
use itertools::Itertools;
use records_lib::ranks::force_update;
use records_lib::{
    event::OptEvent,
    models, must, player,
    ranks::{get_rank, update_leaderboard},
    redis_key::map_key,
    DatabaseConnection,
};
use records_lib::{table_lock, RedisConnection};
use serde::{Deserialize, Serialize};
use tracing_actix_web::RequestId;

use super::map::MapParam;

#[derive(Deserialize)]
pub struct OverviewQuery {
    #[serde(alias = "playerId")]
    pub(crate) login: String,
    #[serde(alias = "mapId")]
    pub(crate) map_uid: String,
}

pub struct OverviewParams<'a> {
    pub(crate) login: String,
    pub(crate) map: MapParam<'a>,
}

impl OverviewQuery {
    #[inline]
    pub fn into_params(self, map: Option<&models::Map>) -> OverviewParams<'_> {
        OverviewParams {
            login: self.login,
            map: MapParam::from_map(map, self.map_uid),
        }
    }
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

async fn get_range(
    conn: &mut DatabaseConnection<'_>,
    (start, end): (u32, u32),
    map_id: u32,
    event: OptEvent<'_, '_>,
) -> RecordsResult<Vec<RankedRecord>> {
    // transforms exclusive to inclusive range
    let end = end - 1;
    let player_ids: Vec<i32> = conn
        .redis_conn
        .zrange(map_key(map_id, event), start as isize, end as isize)
        .await
        .with_api_err()?;

    if player_ids.is_empty() {
        // Avoids the query building to have a `AND record_player_id IN ()` fragment
        return Ok(Vec::new());
    }

    let (join_event, and_event) = event.get_join();

    let params = iter::repeat("?").take(player_ids.len()).join(",");

    let query = format!(
        "SELECT CAST(0 AS UNSIGNED) AS rank,
            p.id AS player_id,
            p.login AS login,
            p.name AS nickname,
            min(time) as time,
            map_id
        FROM records r
        {join_event}
        INNER JOIN players p ON r.record_player_id = p.id
        WHERE map_id = ? AND record_player_id IN ({params})
            {and_event}
        GROUP BY record_player_id
        ORDER BY time, record_date ASC",
    );

    let mut query = sqlx::query_as(&query).bind(map_id);
    for id in player_ids {
        query = query.bind(id);
    }
    if let Some((event, edition)) = event.0 {
        query = query.bind(event.id).bind(edition.id);
    }

    let records: Vec<RecordQueryRow> = query
        .fetch_all(&mut **conn.mysql_conn)
        .await
        .with_api_err()?;

    let mut out = Vec::with_capacity(records.len());

    for r in records {
        out.push(RankedRecord {
            rank: get_rank(conn, map_id, r.player_id, r.time, event).await? as _,
            login: r.login,
            nickname: r.nickname,
            time: r.time,
        });
    }

    Ok(out)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Deserialize))]
pub struct ResponseBody {
    pub response: Vec<RankedRecord>,
}

/// Checks that the time of the player registered in MariaDB and Redis are the same, and updates
/// the Redis leaderboard otherwise.
async fn check_update_redis_lb(
    conn: &mut DatabaseConnection<'_>,
    map_id: u32,
    player_id: u32,
    event: OptEvent<'_, '_>,
) -> RecordsResult<()> {
    // Get the time of the player on this map
    let mdb_time =
        player::get_time_on_map(conn.mysql_conn, player_id, map_id, event.to_ids()).await?;

    // Get the one in Redis
    let r_time: Option<i32> = conn
        .redis_conn
        .zscore(map_key(map_id, event), player_id)
        .await
        .with_api_err()?;

    // Update the Redis leaderboard if they're different
    if mdb_time
        .zip(r_time)
        .is_some_and(|(mdb_time, r_time)| mdb_time != r_time)
    {
        force_update(map_id, event, conn).await.with_api_err()?;
    }

    Ok(())
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

struct BuildRecordsArrayParam<'a> {
    redis_conn: &'a mut RedisConnection,
    map_id: u32,
    player_id: u32,
    event: OptEvent<'a, 'a>,
}

async fn build_records_array(
    mysql_conn: &mut sqlx::pool::PoolConnection<sqlx::MySql>,
    param: BuildRecordsArrayParam<'_>,
) -> Result<Vec<RankedRecord>, Error> {
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn: param.redis_conn,
    };

    // Update redis if needed
    let key = map_key(param.map_id, param.event);
    let count = update_leaderboard(&mut conn, param.map_id, param.event).await? as u32;

    check_update_redis_lb(&mut conn, param.map_id, param.player_id, param.event).await?;

    let mut ranked_records = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: u32 = 15;
    const NO_RECORD_ROWS: u32 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = conn
        .redis_conn
        .zrank(&key, param.player_id)
        .await
        .with_api_err()?;
    let player_rank = player_rank.map(|r| r as u64 as u32);

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            ranked_records
                .extend(get_range(&mut conn, (0, TOTAL_ROWS), param.map_id, param.event).await?);
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            ranked_records.extend(get_range(&mut conn, (0, 3), param.map_id, param.event).await?);

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
            ranked_records.extend(get_range(&mut conn, range, param.map_id, param.event).await?);
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            ranked_records.extend(
                get_range(
                    &mut conn,
                    (0, NO_RECORD_ROWS - 3),
                    param.map_id,
                    param.event,
                )
                .await?,
            );

            // last 3
            ranked_records
                .extend(get_range(&mut conn, (count - 3, count), param.map_id, param.event).await?);
        }
        // There is enough records to display them all
        else {
            ranked_records.extend(
                get_range(&mut conn, (0, NO_RECORD_ROWS), param.map_id, param.event).await?,
            );
        }
    }

    Ok(ranked_records)
}

pub async fn overview(
    req_id: RequestId,
    conn: DatabaseConnection<'_>,
    params: OverviewParams<'_>,
    event: OptEvent<'_, '_>,
) -> RecordsResponse<impl Responder> {
    let models::Map { id, linked_map, .. } = match params.map {
        MapParam::AlreadyQueried(map) => map,
        MapParam::Uid(uid) => &must::have_map(conn.mysql_conn, &uid)
            .await
            .with_api_err()
            .fit(req_id)?,
    };
    let player_id = must::have_player(conn.mysql_conn, &params.login)
        .await
        .fit(req_id)?
        .id;
    let map_id = linked_map.unwrap_or(*id);

    let ranked_records = match table_lock::locked_records(
        conn.mysql_conn,
        BuildRecordsArrayParam {
            redis_conn: conn.redis_conn,
            map_id,
            player_id,
            event,
        },
        build_records_array,
    )
    .await
    {
        Ok(records) => Ok(records),
        Err(Error::Api(e)) => Err(e),
        Err(Error::Lib(e)) => Err(e).with_api_err(),
    }
    .fit(req_id)?;

    let response = ranked_records;
    utils::json(ResponseBody { response })
}
