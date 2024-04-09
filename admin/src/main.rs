use clap::Parser;
use records_lib::{get_mysql_pool, get_redis_pool, Database};

use self::{clear::ClearCommand, populate::PopulateCommand};

mod clear;
mod populate;

#[derive(clap::Parser)]
enum Command {
    #[clap(subcommand)]
    Event(EventCommand),
}

#[derive(clap::Subcommand)]
enum EventCommand {
    Populate(PopulateCommand),
    Clear(ClearCommand),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    tracing_subscriber::fmt()
        .compact()
        .try_init()
        .map_err(|e| anyhow::anyhow!("unable to init tracing_subscriber: {e}"))?;

    let db = Database {
        mysql_pool: get_mysql_pool().await?,
        redis_pool: get_redis_pool()?,
    };

    let cmd = Command::parse();

    let client = reqwest::Client::new();

    match cmd {
        Command::Event(event) => match event {
            EventCommand::Populate(cmd) => populate::populate(client, db, cmd).await?,
            EventCommand::Clear(cmd) => clear::clear(db.mysql_pool, cmd).await?,
        },
    }

    Ok(())
}
