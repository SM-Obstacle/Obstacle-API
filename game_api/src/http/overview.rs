use std::iter;

use actix_web::{web::Query, Responder};
use deadpool_redis::redis::AsyncCommands;
use futures::{Stream, StreamExt};
use itertools::Itertools;
use records_lib::{
    event::OptEvent,
    models, must,
    redis_key::map_key,
    update_ranks::{get_rank, update_leaderboard},
    DatabaseConnection, MySqlPool,
};
use serde::{Deserialize, Serialize};
use tracing_actix_web::RequestId;

use crate::{
    utils::json, FinishLocker, FitRequestId, RecordsResponse, RecordsResult, RecordsResultExt,
};

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
    pub map_id: u32,
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
    db: &sqlx::MySqlPool,
    conn: &mut DatabaseConnection,
    models::Map { id: map_id, .. }: &models::Map,
    (start, end): (u32, u32),
    event: OptEvent<'_, '_>,
) -> RecordsResult<Vec<RankedRecord>> {
    let key = map_key(*map_id, event);

    let (join_event, and_event) = event.get_join();

    // transforms exclusive to inclusive range
    let end = end - 1;
    let ids: Vec<i32> = conn
        .redis_conn
        .zrange(key, start as isize, end as isize)
        .await
        .with_api_err()?;

    if ids.is_empty() {
        // Avoids the query building to have a `AND record_player_id IN ()` fragment
        return Ok(Vec::new());
    }

    let params = iter::repeat("?").take(ids.len()).join(",");

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
    for id in ids {
        query = query.bind(id);
    }

    if let Some((event, edition)) = event.0 {
        query = query.bind(event.id).bind(edition.id);
    }

    let mut records = query.fetch(db);
    let mut out = Vec::with_capacity(records.size_hint().0);

    while let Some(record) = records.next().await {
        let RecordQueryRow {
            login,
            player_id,
            nickname,
            time,
            map_id,
        } = record.with_api_err()?;

        out.push(RankedRecord {
            rank: get_rank(conn, map_id, player_id, time, event).await? as _,
            login,
            nickname,
            time,
        });
    }

    Ok(out)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Deserialize))]
pub struct ResponseBody {
    pub response: Vec<RankedRecord>,
}

pub async fn overview(
    req_id: RequestId,
    locker: FinishLocker,
    db: &MySqlPool,
    conn: &mut DatabaseConnection,
    params: OverviewParams<'_>,
    event: OptEvent<'_, '_>,
) -> RecordsResponse<impl Responder> {
    let map @ models::Map { id, linked_map, .. } = match params.map {
        MapParam::AlreadyQueried(map) => map,
        MapParam::Uid(uid) => &must::have_map(&mut conn.mysql_conn, &uid)
            .await
            .with_api_err()
            .fit(req_id)?,
    };
    let player_id = must::have_player(&mut conn.mysql_conn, &params.login)
        .await
        .fit(req_id)?
        .id;
    let map_id = linked_map.unwrap_or(*id);

    locker.wait_finishes_for(map_id).await;

    // Update redis if needed
    let key = map_key(map_id, event);
    let count = update_leaderboard(conn, map.id, event).await.fit(req_id)? as u32;

    let mut ranked_records: Vec<RankedRecord> = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: u32 = 15;
    const NO_RECORD_ROWS: u32 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = conn
        .redis_conn
        .zrank(&key, player_id)
        .await
        .with_api_err()
        .fit(req_id)?;
    let player_rank = player_rank.map(|r| r as u64 as u32);

    if let Some(player_rank) = player_rank {
        // The player has a record and is in top ROWS, display ROWS records
        if player_rank < TOTAL_ROWS {
            let range = get_range(db, conn, map, (0, TOTAL_ROWS), event)
                .await
                .fit(req_id)?;
            ranked_records.extend(range);
        }
        // The player is not in the top ROWS records, display top3 and then center around the player rank
        else {
            // push top3
            let range = get_range(db, conn, map, (0, 3), event).await.fit(req_id)?;
            ranked_records.extend(range);

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

            let range = get_range(db, conn, map, range, event).await.fit(req_id)?;
            ranked_records.extend(range);
        }
    }
    // The player has no record, so ROWS = ROWS - 1 to keep one last line for the player
    else {
        // There is more than ROWS record + top3,
        // So display all top ROWS records and then the last 3
        if count > NO_RECORD_ROWS {
            // top (ROWS - 1 - 3)
            let range = get_range(db, conn, map, (0, NO_RECORD_ROWS - 3), event)
                .await
                .fit(req_id)?;
            ranked_records.extend(range);

            // last 3
            let range = get_range(db, conn, map, (count - 3, count), event)
                .await
                .fit(req_id)?;
            ranked_records.extend(range);
        }
        // There is enough records to display them all
        else {
            let range = get_range(db, conn, map, (0, NO_RECORD_ROWS), event)
                .await
                .fit(req_id)?;
            ranked_records.extend(range);
        }
    }

    let response = ranked_records;
    json(ResponseBody { response })
}
