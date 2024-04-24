use std::{future::Future, time::Duration};

use anyhow::Context;
use deadpool_redis::Connection;
use mkenv::Env as _;
use records_lib::{DbEnv, LibEnv, MySqlPool, RedisPool};
use sqlx::{pool::PoolConnection, MySql};
use tokio::{task::JoinHandle, time};
use tracing::info;

mod campaign_scores;

async fn handle<F, Fut>(
    mysql_pool: MySqlPool,
    redis_pool: RedisPool,
    period: Duration,
    f: F,
) -> anyhow::Result<()>
where
    F: Fn(PoolConnection<MySql>, Connection) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut interval = time::interval(period);

    loop {
        interval.tick().await;
        let mysql_conn = mysql_pool.acquire().await?;
        let redis_conn = redis_pool.get().await?;
        f(mysql_conn, redis_conn).await?;
    }
}

#[inline]
async fn join<O>(
    task: JoinHandle<anyhow::Result<O>>,
    join_ctx: &'static str,
    task_ctx: &'static str,
) -> anyhow::Result<O> {
    task.await.context(join_ctx)?.context(task_ctx)
}

fn setup_tracing() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .try_init()
        .map_err(|e| anyhow::format_err!("{e}"))
}

mkenv::make_env! {Env includes [DbEnv as db_env, LibEnv as lib_env]:}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    setup_tracing()?;
    let env = Env::try_get()?;
    records_lib::init_env(env.lib_env);

    let mysql_pool = records_lib::get_mysql_pool(env.db_env.db_url.db_url)
        .await
        .context("When creating MySQL pool")?;
    let redis_pool = records_lib::get_redis_pool(env.db_env.redis_url.redis_url).context("When creating Redis pool")?;

    let res = tokio::spawn(handle(
        mysql_pool.clone(),
        redis_pool.clone(),
        campaign_scores::PROCESS_DURATION,
        campaign_scores::update,
    ));

    info!("Spawned all tasks");

    join(
        res,
        "When joining the campaign_scores::update task",
        "When updating campaign scores",
    )
    .await?;

    Ok(())
}
