use std::{
    cmp::Ordering,
    fmt,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    str::FromStr,
    time::Instant,
};

use anyhow::Context as _;
use chrono::{DateTime, Days, Months, Utc};
use clap::Parser as _;
use mkenv::Env as _;
use records_lib::{DbUrlEnv, time::Time};
use sea_orm::Database;

mkenv::make_env! {AppEnv includes [DbUrlEnv as db_env]:}

#[derive(Clone)]
struct SinceDuration {
    date: DateTime<Utc>,
}

#[derive(Debug)]
struct InvalidSinceDuration;

impl fmt::Display for InvalidSinceDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid \"since duration\" argument")
    }
}

impl std::error::Error for InvalidSinceDuration {}

impl FromStr for SinceDuration {
    type Err = InvalidSinceDuration;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (n, unit) = s.split_at(s.len() - 1);
        let n = n.parse::<u32>().map_err(|_| InvalidSinceDuration)?;
        let now = Utc::now();
        let date = match unit {
            "d" => now - Days::new(n as _),
            "w" => now - Days::new(n as u64 * 7),
            "m" => now - Months::new(n),
            "y" => now - Months::new(n * 12),
            _ => return Err(InvalidSinceDuration),
        };
        Ok(Self { date })
    }
}

#[derive(clap::Parser)]
struct Args {
    #[arg(
        short = 'p',
        long = "player-file",
        default_value = "player_ranking.csv"
    )]
    player_ranking_file: String,
    #[arg(short = 'm', long = "map-file", default_value = "map_ranking.csv")]
    map_ranking_file: String,
    #[arg(long = "since", value_parser = clap::value_parser!(SinceDuration))]
    from_date: Option<SinceDuration>,
}

fn open_file<P: AsRef<Path>>(path: P) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let now = Instant::now();

    dotenvy::dotenv().context("couldn't get environment file")?;
    let args = Args::parse();

    let mut player_ranking_file =
        open_file(args.player_ranking_file).context("couldn't open player ranking output file")?;
    let mut map_ranking_file =
        open_file(args.map_ranking_file).context("couldn't open map ranking output file")?;

    player_ranking_file
        .write(b"id,login,name,score,player_link\n")
        .context("couldn't write header to player ranking file")?;
    map_ranking_file
        .write(b"id,map_uid,name,score,average_score,min_record,")
        .and_then(|_| {
            map_ranking_file
                .write(b"max_record,average_record,median_record,records_count,map_link\n")
        })
        .context("couldn't write header to map ranking file")?;

    let db_url = AppEnv::try_get()
        .context("couldn't initialize environment")?
        .db_env
        .db_url;
    let db = Database::connect(db_url)
        .await
        .context("couldn't connect to database")?;

    println!(
        "Calculating scores{}...",
        match &args.from_date {
            Some(SinceDuration { date }) => format!(" since {}", date.format("%d/%m/%Y")),
            None => "".to_owned(),
        }
    );

    let scores = player_map_ranking::compute_scores(&db, args.from_date.map(|d| d.date))
        .await
        .context("couldn't compute the scores")?;

    println!("Sorting them...");

    let mut player_ranking = scores.player_scores.into_iter().collect::<Vec<_>>();
    player_ranking.sort_by(|(_, a), (_, b)| {
        b.partial_cmp(a).unwrap_or(Ordering::Equal)
    });
    let mut map_ranking = scores.map_scores.into_iter().collect::<Vec<_>>();
    map_ranking.sort_by(|(_, a), (_, b)| {
        b.partial_cmp(a).unwrap_or(Ordering::Equal)
    });

    println!("Writing to files...");

    for (player, score) in player_ranking {
        writeln!(
            player_ranking_file,
            "{},{login},{},{score},https://obstacle.titlepack.io/player/{login}",
            player.inner.id,
            player.inner.name,
            login = player.inner.login,
        )
        .context("couldn't write a row to player ranking file")?;
    }

    for (map, score) in map_ranking {
        writeln!(
            map_ranking_file,
            "{},{map_uid},{},{score},{},{},{},{},{},{},https://obstacle.titlepack.io/map/{map_uid}",
            map.inner.id,
            map.inner.name,
            score / map.stats.records_count,
            map.stats.min_record,
            map.stats.max_record,
            map.stats.average_record,
            map.stats.median_record,
            map.stats.records_count,
            map_uid = map.inner.game_id,
        )
        .context("couldn't write a row to map ranking file")?;
    }

    println!(
        "Finished. Time taken: {}",
        Time(now.elapsed().as_millis() as _)
    );

    Ok(())
}
