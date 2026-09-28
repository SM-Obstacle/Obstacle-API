use deadpool_redis::redis::{self, AsyncCommands as _, Pipeline};
use records_lib::Database;

pub async fn clear(db: Database) -> anyhow::Result<()> {
    let mut redis_conn = db.redis_pool.get().await?;

    let keys: Vec<String> = redis_conn.keys("v3:mappack*").await?;

    let n = keys.len();

    let mut pipe = redis::pipe();
    keys.into_iter()
        .fold(&mut pipe, Pipeline::del)
        .exec_async(&mut redis_conn)
        .await?;

    tracing::info!("Removed {n} key{}", if n > 0 { "s" } else { "" });

    Ok(())
}
