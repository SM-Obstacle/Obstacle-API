//! The ShootMania Obstacle Cache Manager (SOCC)
//!
//! The cache manager is a service that runs alongside the API. It fetches periodically the records
//! in the database to calculate the scores in certain contexts, then store the results in the Redis
//! database.
//!
//! The jobs are planned with cron expressions, with a seconds field, and are in UTC. See the
//! [`scheduler`] module for more information.

use cron::Schedule;
use mkenv::prelude::*;
use records_lib::{Database, DbEnv, LibEnv};

mod campaign_scores;
mod player_map_ranking;
mod scheduler;

use campaign_scores::CampaignScores;
use player_map_ranking::{GlobalRanking, PeriodicRanking};
use scheduler::Scheduler;

fn setup_tracing() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .try_init()
        .map_err(|e| anyhow::format_err!("{e}"))
}

fn default_schedule(expr: &str) -> Schedule {
    expr.parse()
        .expect("default cron expression should be valid")
}

mkenv::make_config! {
    struct SchedulesEnv {
        periodic_ranking: {
            var_name: "SOCC_PERIODIC_RANKING_SCHEDULE",
            layers: [
                parsed_from_str<Schedule>(),
                or_default_val(|| default_schedule("0 0 0 * * Mon")),
            ],
            description: "The cron schedule of the update of the player/map ranking scores. \
                Each update uses the records made during the period preceding the last record, \
                the period being the time between two occurrences of the schedule",
            default_val_fmt: "0 0 0 * * Mon (every Monday at midnight)",
        },

        global_ranking: {
            var_name: "SOCC_GLOBAL_RANKING_SCHEDULE",
            layers: [
                parsed_from_str<Schedule>(),
                or_default_val(|| default_schedule("0 0 0 1 * *")),
            ],
            description: "The cron schedule of the update of the global (all-time) player/map \
                ranking scores. The periodic ranking uses them to order the players/maps that \
                weren't active during the last period",
            default_val_fmt: "0 0 0 1 * * (the first day of every month at midnight)",
        }
    }
}

mkenv::make_config! {
    struct Env {
        db_env: { DbEnv },
        lib_env: { LibEnv },
        schedules: { SchedulesEnv },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The .env file is optional: the environment can also be set by the container runtime.
    if let Err(e) = dotenvy::dotenv()
        && !e.not_found()
    {
        return Err(e.into());
    }
    setup_tracing()?;
    let env = Env::define();
    env.init();
    let schedules = env.schedules;
    let campaign_scores_schedule = env.lib_env.event_scores_schedule.get();
    records_lib::init_env(env.lib_env);

    let db = Database::from_db_url(
        env.db_env.db_url.db_url.get(),
        env.db_env.redis_url.redis_url.get(),
    )
    .await?;

    let mut scheduler = Scheduler::default();
    scheduler
        .add("campaign_scores", campaign_scores_schedule, CampaignScores)
        // Both ranking jobs share the same intermediate tables, so they must not overlap. The
        // global one is registered first, so that the periodic one uses fresh global scores when
        // both are due at the same time, which is notably the case at startup.
        .add("ranking", schedules.global_ranking.get(), GlobalRanking)
        .add("ranking", schedules.periodic_ranking.get(), PeriodicRanking);

    scheduler.run(db).await
}
