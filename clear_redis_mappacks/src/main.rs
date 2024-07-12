use deadpool_redis::redis::AsyncCommands;
use mkenv::Env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    tracing_subscriber::fmt().init();

    let redis_env = records_lib::RedisUrlEnv::try_get()?;
    let redis_pool = records_lib::get_redis_pool(redis_env.redis_url)?;

    let mut redis_conn = redis_pool.get().await?;
    let keys: Vec<String> = redis_conn.keys("v3:mappack*").await?;

    let n = keys.len();

    for key in keys {
        redis_conn.del(key).await?;
    }

    tracing::info!("Removed {n} key{}", if n > 0 { "s" } else { "" });

    Ok(())
}
