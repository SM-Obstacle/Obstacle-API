use std::fmt;

use deadpool_redis::redis::AsyncCommands;
use futures::StreamExt;
use records_lib::{
    map, must, player, redis_key::map_key, time::Time, update_ranks::get_rank, Database,
    DatabaseConnection,
};

#[derive(clap::Subcommand)]
pub enum LbCommand {
    Full(FullCmd),
}

#[derive(clap::ValueEnum, Default, Clone, Copy)]
enum LbSource {
    #[value(name = "mariadb", alias = "sql", alias = "mysql")]
    MariaDb,
    #[value(name = "redis")]
    #[default]
    Redis,
}

#[derive(clap::Args)]
#[clap(name = "full")]
pub struct FullCmd {
    /// The source of the leaderboard.
    #[arg(value_enum, long, short, default_value_t)]
    source: LbSource,

    #[arg(long)]
    offset: Option<isize>,

    #[arg(long, short = 'n')]
    limit: Option<isize>,

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

#[derive(sqlx::FromRow)]
struct LbLine {
    rank: i32,
    player: String,
    time: i32,
}

async fn mariadb_lb(
    db: &mut DatabaseConnection,
    map_id: u32,
    offset: Option<isize>,
    limit: Option<isize>,
) -> anyhow::Result<()> {
    struct Offset(Option<isize>);

    impl fmt::Display for Offset {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Some(offset) => write!(f, "offset {offset}"),
                None => Ok(()),
            }
        }
    }

    struct Limit(Option<isize>);

    impl fmt::Display for Limit {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Some(limit) => write!(f, "limit {limit}"),
                None => Ok(()),
            }
        }
    }

    let query = &format!(
        "select
            rank() over (order by r.time) as rank,
            p.login as player,
            r.time as time
        from global_records r
        inner join players p on p.id = r.record_player_id
        where r.map_id = ?
        order by r.time
        {limit} {offset}",
        limit = Limit(limit),
        offset = Offset(offset),
    );

    let mut lb = sqlx::query_as(query)
        .bind(map_id)
        .fetch(&mut *db.mysql_conn)
        .enumerate()
        .map(|(i, line)| (i + 1, line));

    let mut table = prettytable::Table::new();
    table.add_row(prettytable::row!["#", "Rank", "Player", "Time"]);

    while let Some((i, lb)) = lb.next().await {
        let LbLine { rank, player, time } = lb?;
        table.add_row(prettytable::row![
            i + offset.unwrap_or_default() as usize,
            rank,
            player,
            Time(time)
        ]);
    }

    println!("Source: MariaDB");
    println!("{table}");

    Ok(())
}

async fn redis_lb(
    db: &mut DatabaseConnection,
    map_id: u32,
    offset: Option<isize>,
    limit: Option<isize>,
) -> anyhow::Result<()> {
    let ranks: Vec<i64> = db
        .redis_conn
        .zrange_withscores(
            map_key(map_id, Default::default()),
            offset.unwrap_or_default(),
            limit.unwrap_or(-1),
        )
        .await?;

    let mut table = prettytable::Table::new();

    table.add_row(prettytable::row!["#", "Rank", "Player", "Time"]);

    for (i, row) in ranks.chunks_exact(2).enumerate().map(|(i, r)| (i + 1, r)) {
        let [player_id, time] = *row else {
            continue;
        };

        let player = player::get_player_from_id(&mut *db.mysql_conn, player_id as _).await?;
        let rank = get_rank(db, map_id, player_id as _, Default::default()).await?;

        table.add_row(prettytable::row![
            i + offset.unwrap_or_default() as usize,
            rank,
            player.login,
            Time(time as _)
        ]);
    }

    println!("Source: Redis");
    println!("{table}");

    Ok(())
}

async fn full(db: &mut DatabaseConnection, cmd: FullCmd) -> anyhow::Result<()> {
    let map = match cmd.map {
        Map::MapId { map_id } => map::get_map_from_id(&mut db.mysql_conn, map_id).await?,
        Map::MapUid { map_uid } => must::have_map(&mut db.mysql_conn, &map_uid).await?,
    };

    match cmd.source {
        LbSource::MariaDb => mariadb_lb(db, map.id, cmd.offset, cmd.limit).await,
        LbSource::Redis => redis_lb(db, map.id, cmd.offset, cmd.limit).await,
    }
}

pub async fn leaderboard(db: Database, cmd: LbCommand) -> anyhow::Result<()> {
    let mut conn = db.acquire().await?;

    match cmd {
        LbCommand::Full(full_cmd) => full(&mut conn, full_cmd).await?,
    }

    conn.close().await?;

    Ok(())
}
