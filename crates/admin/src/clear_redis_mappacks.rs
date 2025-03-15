use deadpool_redis::redis::AsyncCommands as _;
use records_lib::Database;

pub async fn clear(db: Database) -> anyhow::Result<()> {
    let mut redis_conn = db.redis_pool.get().await?;

    let keys: Vec<String> = redis_conn.keys("v3:mappack*").await?;

    let n = keys.len();

    for key in keys {
        let _: () = redis_conn.del(key).await?;
    }

    tracing::info!("Removed {n} key{}", if n > 0 { "s" } else { "" });

    Ok(())
}
