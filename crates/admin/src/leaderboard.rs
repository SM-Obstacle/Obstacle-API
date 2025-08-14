use records_lib::{Database, RedisConnection, leaderboard, map, must, time::Time};
use sea_orm::{ConnectionTrait, DatabaseConnection, StreamTrait};

#[derive(clap::Subcommand)]
pub enum LbCommand {
    Full(FullCmd),
}

#[derive(clap::Args)]
#[clap(name = "full")]
pub struct FullCmd {
    #[arg(long)]
    offset: Option<i64>,

    #[arg(long, short = 'n')]
    limit: Option<i64>,

    #[clap(subcommand)]
    map: Map,
}

#[derive(clap::Subcommand)]
enum Map {
    MapId {
        /// The map ID.
        map_id: u32,
    },

    MapUid {
        /// The map UID.
        map_uid: String,
    },
}

async fn mariadb_lb<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    offset: Option<i64>,
    limit: Option<i64>,
) -> anyhow::Result<()> {
    let leaderboard =
        leaderboard::leaderboard(conn, redis_conn, map_id, offset, limit, Default::default())
            .await?
            .into_iter()
            .enumerate();

    let mut table =
        prettytable::Table::init(vec![prettytable::row!["#", "Rank", "Player", "Time"]]);

    for (i, row) in leaderboard {
        table.add_row(prettytable::row![
            i + offset.unwrap_or_default() as usize,
            row.rank,
            row.login,
            Time(row.time)
        ]);
    }

    println!("Source: MariaDB");
    println!("{table}");

    Ok(())
}

async fn full<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    cmd: FullCmd,
) -> anyhow::Result<()> {
    let map = match cmd.map {
        Map::MapId { map_id } => map::get_map_from_id(conn, map_id).await?,
        Map::MapUid { map_uid } => must::have_map(conn, &map_uid).await?,
    };
    mariadb_lb(conn, redis_conn, map.id, cmd.offset, cmd.limit).await
}

pub async fn leaderboard(db: Database, cmd: LbCommand) -> anyhow::Result<()> {
    let conn = DatabaseConnection::from(db.mysql_pool);
    let mut redis_conn = db.redis_pool.get().await?;

    match cmd {
        LbCommand::Full(full_cmd) => full(&conn, &mut redis_conn, full_cmd).await?,
    }

    Ok(())
}
