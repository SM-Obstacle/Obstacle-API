//! The ShootMania Obstacle Cache Manager (SOCC)
//!
//! The cache manager is a service that runs alongside the API. It fetches periodically the records
//! in the database to calculate the scores in certain contexts, then store the results in the Redis
//! database.

use std::{future::Future, time::Duration};

use anyhow::Context;
use mkenv::prelude::*;
use records_lib::{Database, DbEnv, LibEnv};
use tokio::{task::JoinHandle, time};
use tracing::info;

mod campaign_scores;
mod player_ranking;

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

mkenv::make_config! {
    struct Env {
        db_env: { DbEnv },
        lib_env: { LibEnv },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    setup_tracing()?;
    let env = Env::define();
    env.init();
    let event_scores_interval = env.lib_env.event_scores_interval.get();
    let player_ranking_scores_interval = env.lib_env.player_map_ranking_scores_interval.get();
    records_lib::init_env(env.lib_env);

    let db = Database::from_db_url(
        env.db_env.db_url.db_url.get(),
        env.db_env.redis_url.redis_url.get(),
    )
    .await?;

    let event_scores_handle = tokio::spawn(handle(
        db.clone(),
        event_scores_interval,
        campaign_scores::update,
    ));

    let player_map_ranking_handle = tokio::spawn(handle(
        db.clone(),
        player_ranking_scores_interval,
        move |db| {
            player_ranking::update(
                db,
                Some(chrono::Utc::now() - player_ranking_scores_interval),
            )
        },
    ));

    info!("Spawned all tasks");

    join(
        event_scores_handle,
        "When joining the campaign_scores::update task",
        "When updating campaign scores",
    )
    .await?;

    join(
        player_map_ranking_handle,
        "When joining the player_ranking::update task",
        "When updating player and map ranking scores",
    )
    .await?;

    Ok(())
}
