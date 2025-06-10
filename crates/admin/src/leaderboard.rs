use records_lib::{Database, DatabaseConnection, acquire, leaderboard, map, must, time::Time};

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

async fn mariadb_lb(
    db: &mut DatabaseConnection<'_>,
    map_id: u32,
    offset: Option<i64>,
    limit: Option<i64>,
) -> anyhow::Result<()> {
    let leaderboard = leaderboard::leaderboard(db, Default::default(), map_id, offset, limit)
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

async fn full(db: &mut DatabaseConnection<'_>, cmd: FullCmd) -> anyhow::Result<()> {
    let map = match cmd.map {
        Map::MapId { map_id } => map::get_map_from_id(db.mysql_conn, map_id).await?,
        Map::MapUid { map_uid } => must::have_map(db.mysql_conn, &map_uid).await?,
    };
    mariadb_lb(db, map.id, cmd.offset, cmd.limit).await
}

pub async fn leaderboard(db: Database, cmd: LbCommand) -> anyhow::Result<()> {
    let mut conn = acquire!(db?);

    match cmd {
        LbCommand::Full(full_cmd) => full(&mut conn, full_cmd).await?,
    }

    Ok(())
}
