use crate::{models::Record, Database, RecordsResult};
use deadpool_redis::redis::AsyncCommands;

pub async fn count_records_map(db: &Database, map_id: u32) -> RecordsResult<i64> {
    sqlx::query_scalar!("SELECT COUNT(*) FROM records WHERE map_id = ?", map_id)
        .fetch_one(&db.mysql_pool)
        .await
        .map_err(|e| e.into())
}

pub async fn update_leaderboard(db: &Database, key: &str, map_id: u32) -> RecordsResult<i64> {
    let mut redis_conn = db.redis_pool.get().await.unwrap();
    let redis_count: i64 = redis_conn.zcount(key, "-inf", "+inf").await.unwrap();
    let mysql_count: i64 = count_records_map(db, map_id).await?;

    if redis_count != mysql_count {
        let all_map_records =
            sqlx::query_as!(Record, "SELECT * FROM records WHERE map_id = ?", map_id)
                .fetch_all(&db.mysql_pool)
                .await?;

        let _removed_count: i64 = redis_conn.del(key).await.unwrap_or(0);

        for record in all_map_records {
            let _: i64 = redis_conn
                .zadd(key, record.player_id, record.time)
                .await
                .unwrap_or(0);
        }
    }

    Ok(mysql_count)
}
