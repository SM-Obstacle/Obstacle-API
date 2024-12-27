//! The ShootMania Obstacle Cache Manager (SOCC)
//!
//! The cache manager is a service that runs alongside the API. It fetches periodically the records
//! in the database to calculate the scores in certain contexts, then store the results in the Redis
//! database.

use std::{future::Future, time::Duration};

use anyhow::Context;
use mkenv::Env as _;
use records_lib::{Database, DbEnv, LibEnv};
use tokio::{task::JoinHandle, time};
use tracing::info;

mod campaign_scores;

async fn handle<F, Fut>(db: Database, period: Duration, f: F) -> anyhow::Result<()>
where
    F: Fn(Database) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut interval = time::interval(period);

    loop {
        interval.tick().await;
        f(db.clone()).await?;
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

    let mysql_pool = records_lib::create_mysql_pool(env.db_env.db_url.db_url)
        .await
        .context("When creating MySQL pool")?;
    let redis_pool = records_lib::create_redis_pool(env.db_env.redis_url.redis_url)
        .context("When creating Redis pool")?;

    let db = Database {
        mysql_pool,
        redis_pool,
    };

    let res = tokio::spawn(handle(
        db.clone(),
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
