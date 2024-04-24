use clap::Parser;
use mkenv::Env as _;
use records_lib::{get_mysql_pool, get_redis_pool, Database, DbEnv, LibEnv};

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

mkenv::make_env!(Env includes [DbEnv as db_env, LibEnv as lib_env]:);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    tracing_subscriber::fmt()
        .compact()
        .try_init()
        .map_err(|e| anyhow::anyhow!("unable to init tracing_subscriber: {e}"))?;
    let env = Env::try_get()?;
    records_lib::init_env(env.lib_env);

    let db = Database {
        mysql_pool: get_mysql_pool(env.db_env.db_url.db_url).await?,
        redis_pool: get_redis_pool(env.db_env.redis_url.redis_url)?,
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
