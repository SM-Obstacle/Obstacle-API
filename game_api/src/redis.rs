use crate::{http::event, models, utils::format_map_key, RecordsResult};
use deadpool_redis::{redis::AsyncCommands, Connection as RedisConnection};
use sqlx::{Executor, MySql, MySqlConnection};

async fn count_records_map<'c, E: Executor<'c, Database = MySql>>(
    db: E,
    map_id: u32,
) -> RecordsResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
        FROM (SELECT * FROM records
        WHERE map_id = ?
        GROUP BY record_player_id) r",
    )
    .bind(map_id)
    .fetch_one(db)
    .await
    .map_err(|e| e.into())
}

/// Checks if the Redis leaderboard for the map with the `key` has a different count
/// that in the database, and reupdates the Redis leaderboard completly if so.
///
/// This is a check to avoid records duplicates, that may happen sometimes.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard(
    (db, redis_conn): (&mut MySqlConnection, &mut RedisConnection),
    models::Map {
        id: map_id,
        reversed,
        ..
    }: &models::Map,
    event: Option<&(models::Event, models::EventEdition)>,
) -> RecordsResult<i64> {
    let reversed_lb = reversed.unwrap_or(false);
    let key = format_map_key(*map_id, event);
    let redis_count: i64 = redis_conn.zcount(&key, "-inf", "+inf").await?;
    let mysql_count: i64 = count_records_map(&mut *db, *map_id).await?;

    let (join_event, and_event) = event
        .is_some()
        .then(event::get_sql_fragments)
        .unwrap_or_default();

    if redis_count != mysql_count {
        let query = format!(
            "SELECT record_player_id, {}(time) AS time
            FROM records r
            {join_event}
                WHERE map_id = ?
                    {and_event}
                GROUP BY record_player_id
                ORDER BY time {}, record_date ASC",
            if reversed_lb { "MAX" } else { "MIN" },
            if reversed_lb { "DESC" } else { "ASC" },
        );

        let mut query = sqlx::query_as(&query).bind(map_id);

        if let Some((event, edition)) = event {
            query = query.bind(event.id).bind(edition.id);
        }

        let all_map_records: Vec<(u32, i32)> = query.fetch_all(db).await?;

        let _removed_count: i64 = redis_conn.del(&key).await?;

        for record in all_map_records {
            let _: i64 = redis_conn.zadd(&key, record.0, record.1).await?;
        }
    }

    Ok(mysql_count)
}
