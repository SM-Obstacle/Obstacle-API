use records_lib::models::*;
use sqlx::mysql;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Duration;

struct MapStats {
    pub records_count: f64,
    pub min_record: f64,
    pub average_record: f64,
    pub median_record: f64,
    pub max_record: f64,
}

impl MapStats {
    pub fn new() -> Self {
        MapStats {
            records_count: 0.0,
            min_record: 0.0,
            average_record: 0.0,
            median_record: 0.0,
            max_record: 0.0,
        }
    }
}

fn compute_score(r: f64, rn: f64, t: f64, average_record: f64) -> f64 {
    let record_score = (1000.0 * (rn * rn)).log10() + ((average_record - t).powi(2) + 1.0).log10();
    let record_score = record_score * ((rn / r) + 1.0).log10().powi(3);
    record_score
}

async fn compute_map_score(
    mysql_pool: &mysql::MySqlPool,
    map_stats: &HashMap<u32, MapStats>,
    map_id: u32,
) -> f64 {
    let stats = &map_stats[&map_id];
    let map_records = sqlx::query_as!(
        Record,
        "SELECT * from records WHERE map_id = ? ORDER BY time",
        map_id
    )
    .fetch_all(mysql_pool)
    .await
    .unwrap();
    let to_sec = |time: i32| (time as f64) / 1000.0;

    let r = 1.0;
    let rn = stats.records_count;
    let t = to_sec(map_records[0].time);
    let t = t.max(stats.average_record);

    compute_score(r, rn, t, stats.average_record)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mysql_pool = mysql::MySqlPoolOptions::new()
        .connect_timeout(Duration::new(10, 0))
        .connect("mysql://root:root@localhost/obstacle_records")
        .await?;

    let maps: HashMap<u32, Map> = sqlx::query_as!(Map, "SELECT * FROM maps")
        .map(|map| (map.id, map))
        .fetch_all(&mysql_pool)
        .await?
        .into_iter()
        .collect();

    let players: HashMap<u32, Player> = sqlx::query_as!(Player, "SELECT * FROM players")
        .map(|player| (player.id, player))
        .fetch_all(&mysql_pool)
        .await?
        .into_iter()
        .collect();

    let mut map_stats: HashMap<u32, MapStats> = HashMap::new();
    let mut map_scores: HashMap<u32, f64> = HashMap::new();
    let mut player_scores: HashMap<u32, f64> = HashMap::new();

    let to_sec = |time: i32| (time as f64) / 1000.0;

    for (_, map) in &maps {
        let map_records = sqlx::query_as!(
            Record,
            "SELECT * from records WHERE map_id = ? ORDER BY time",
            map.id
        )
        .fetch_all(&mysql_pool)
        .await?;

        // Skip maps without records
        if map_records.is_empty() {
            continue;
        }

        // Compute map stats
        let mut stats = MapStats::new();
        stats.records_count = map_records.len() as f64;
        stats.min_record = to_sec(map_records[0].time);
        stats.max_record = to_sec(map_records[0].time);
        for record in &map_records {
            stats.min_record = stats.min_record.min(to_sec(record.time));
            stats.max_record = stats.max_record.max(to_sec(record.time));
            stats.average_record += to_sec(record.time);
        }
        stats.average_record = stats.average_record / stats.records_count;
        stats.median_record = to_sec(map_records[map_records.len() / 2].time);

        // Compute score
        for i_record in 0..map_records.len() {
            let record = &map_records[i_record];

            let r = (i_record + 1) as f64;
            let rn = map_records.len() as f64;
            let t = to_sec(record.time);
            let t = t.max(stats.average_record);

            let record_score = compute_score(r, rn, t, stats.average_record);

            *map_scores.entry(record.map_id).or_insert(0.0) += record_score;
            *player_scores.entry(record.player_id).or_insert(0.0) += record_score;
        }

        map_stats.insert(map.id, stats);
    }

    let id = 16284;
    let map = &maps[&id];
    println!(
        "r1 for map #{} \"{}\": {} pts of {} total.",
        map.id,
        map.name,
        compute_map_score(&mysql_pool, &map_stats, map.id).await,
        &map_scores[&id]
    );

    let id = 38179;
    let map = &maps[&id];
    println!(
        "r1 for map #{} \"{}\": {} pts of {} total.",
        map.id,
        map.name,
        compute_map_score(&mysql_pool, &map_stats, map.id).await,
        &map_scores[&id]
    );

    let mut player_scores = player_scores.into_iter().collect::<Vec<_>>();
    player_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut map_scores = map_scores.into_iter().collect::<Vec<_>>();
    map_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut player_ladder = File::create("player_ladder.csv")?;
    player_ladder.write_all(b"id,login,name,score\n")?;
    for (player_id, score) in &player_scores {
        let player = &players[&player_id];
        write!(
            &mut player_ladder,
            "{},{},{},{}\n",
            player_id, player.login, player.name, score
        )?;
    }

    let mut map_ladder = File::create("map_ladder.csv")?;
    map_ladder.write_all(b"id,name,score,average_score,min_record,max_record,average_record,median_record,records_count\n")?;
    for (map_id, score) in &map_scores {
        let map = &maps[&map_id];
        let stats = &map_stats[&map_id];
        let average = score / (stats.records_count as f64);
        write!(
            &mut map_ladder,
            "{},{},{},{},{},{},{},{},{}\n",
            map_id,
            map.name,
            score,
            average,
            stats.min_record,
            stats.max_record,
            stats.average_record,
            stats.median_record,
            stats.records_count
        )?;
    }

    println!(
        "Computed score for {} players and {} maps.",
        player_scores.len(),
        map_scores.len()
    );

    Ok(())
}
