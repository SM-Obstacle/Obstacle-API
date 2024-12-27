use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use deadpool_redis::redis::AsyncCommands;
use records_lib::context::{
    Ctx, HasMap, HasMapId, HasPersistentMode, HasPlayerLogin, ReadOnly, Transactional,
};
use records_lib::{player, ranks, transaction, MySqlConnection, RedisConnection};
use records_lib::{ranks::get_rank, redis_key::map_key, DatabaseConnection};
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

async fn extend_range<C>(
    conn: &mut DatabaseConnection<'_>,
    records: &mut Vec<RankedRecord>,
    (start, end): (u32, u32),
    ctx: C,
) -> RecordsResult<()>
where
    C: HasMapId + Transactional,
{
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
            " INNER JOIN players p ON r.record_player_id = p.id
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

async fn build_records_array<C>(
    mysql_conn: MySqlConnection<'_>,
    ctx: C,
    redis_conn: &mut RedisConnection,
) -> RecordsResult<Vec<RankedRecord>>
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

    let count = ranks::count_records_map(conn.mysql_conn, &ctx).await?;
    let key = map_key(ctx.get_map().id, ctx.get_opt_event_edition());

    let mut ranked_records = vec![];

    // -- Compute display ranges
    const TOTAL_ROWS: u32 = 15;
    const NO_RECORD_ROWS: u32 = TOTAL_ROWS - 1;

    let player_rank: Option<i64> = match player {
        Some(ref player) => conn
            .redis_conn
            .zrank(&key, player.id)
            .await
            .with_api_err()?,
        None => None,
    };
    let player_rank = player_rank.map(|r| r as u64 as u32);

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
                if end >= count {
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
    let ranked_records = transaction::within(
        conn.mysql_conn,
        ctx,
        ReadOnly,
        conn.redis_conn,
        build_records_array,
    )
    .await?;

    Ok(ResponseBody {
        response: ranked_records,
    })
}
