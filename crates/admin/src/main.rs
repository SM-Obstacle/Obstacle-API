use clap::Parser;
use mkenv::prelude::*;
use records_lib::{Database, DbEnv, LibEnv};

use self::{clear::ClearCommand, leaderboard::LbCommand, populate::PopulateCommand};

mod clear;
mod clear_redis_mappacks;
mod leaderboard;
mod populate;

#[derive(clap::Parser)]
enum Command {
    #[clap(subcommand)]
    Event(EventCommand),
    #[clap(subcommand)]
    Leaderboard(LbCommand),
    ClearRedisMappacks,
}

#[derive(clap::Subcommand)]
enum EventCommand {
    Populate(PopulateCommand),
    Clear(ClearCommand),
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
    tracing_subscriber::fmt()
        .compact()
        .try_init()
        .map_err(|e| anyhow::anyhow!("unable to init tracing_subscriber: {e}"))?;
    let env = Env::define();
    env.init();
    records_lib::init_env(env.lib_env);

    let db = Database::from_db_url(
        env.db_env.db_url.db_url.get(),
        env.db_env.redis_url.redis_url.get(),
    )
    .await?;

    let cmd = Command::parse();

    let client = reqwest::Client::new();

    match cmd {
        Command::Event(event) => match event {
            EventCommand::Populate(cmd) => populate::populate(client, db, cmd).await,
            EventCommand::Clear(cmd) => clear::clear(db, cmd).await,
        },
        Command::Leaderboard(cmd) => leaderboard::leaderboard(db, cmd).await,
        Command::ClearRedisMappacks => clear_redis_mappacks::clear(db).await,
    }
}
