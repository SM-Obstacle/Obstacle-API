use std::{collections::HashMap, hash::Hash};

use anyhow::Context as _;
use entity::{global_records, maps, players};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _, QueryOrder, QuerySelect,
    QueryTrait,
    prelude::Expr,
    sea_query::Func,
    sqlx::types::chrono::{DateTime, Utc},
};

#[derive(Default)]
pub struct MapStats {
    pub records_count: f64,
    pub min_record: f64,
    pub average_record: f64,
    pub median_record: f64,
    pub max_record: f64,
}

fn ms_to_sec(time: i32) -> f64 {
    time as f64 / 1000.
}

fn compute_score(r: f64, rn: f64, t: f64, average_record: f64) -> f64 {
    let record_score = (1000.0 * (rn * rn)).log10() + ((average_record - t).powi(2) + 1.0).log10();
    record_score * ((rn / r) + 1.0).log10().powi(3)
}

#[derive(sea_orm::FromQueryResult)]
struct RawPlayer {
    #[sea_orm(nested)]
    inner: players::Model,
    unstyled_name: String,
}

#[derive(sea_orm::FromQueryResult)]
struct RawMap {
    #[sea_orm(nested)]
    inner: maps::Model,
    unstyled_name: String,
}

pub struct HashablePlayer {
    pub inner: players::Model,
    pub unstyled_name: String,
}

impl Hash for HashablePlayer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.id.hash(state);
    }
}

impl PartialEq for HashablePlayer {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for HashablePlayer {}

pub struct HashableMap {
    pub inner: maps::Model,
    pub stats: MapStats,
    pub unstyled_name: String,
}

impl Hash for HashableMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.id.hash(state);
    }
}

impl PartialEq for HashableMap {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for HashableMap {}

pub struct Scores {
    pub player_scores: HashMap<HashablePlayer, f64>,
    pub map_scores: HashMap<HashableMap, f64>,
}

pub async fn compute_scores<C: ConnectionTrait>(
    conn: &C,
    from: Option<DateTime<Utc>>,
) -> anyhow::Result<Scores> {
    let mut maps = maps::Entity::find()
        .expr_as(
            Func::cust("unstyled").arg(Expr::col((maps::Entity, maps::Column::Name))),
            "unstyled_name",
        )
        .into_model::<RawMap>()
        .all(conn)
        .await
        .context("couldn't retrieve all maps")?
        .into_iter()
        .map(|map| (map.inner.id, map))
        .collect::<HashMap<_, _>>();

    let mut players = players::Entity::find()
        .expr_as(
            Func::cust("unstyled").arg(Expr::col((players::Entity, players::Column::Name))),
            "unstyled_name",
        )
        .into_model::<RawPlayer>()
        .all(conn)
        .await
        .context("couldn't retrieve all players")?
        .into_iter()
        .map(|player| (player.inner.id, player))
        .collect::<HashMap<_, _>>();

    let mut map_stats = HashMap::<u32, MapStats>::new();
    let mut map_scores = HashMap::<u32, f64>::new();
    let mut player_scores = HashMap::<u32, f64>::new();

    for map in maps.values() {
        let map_records = global_records::Entity::find()
            .filter(global_records::Column::MapId.eq(map.inner.id))
            .apply_if(from, |query, from| {
                query.filter(global_records::Column::RecordDate.gte(from))
            })
            .order_by_asc(global_records::Column::Time)
            .all(conn)
            .await
            .with_context(|| format!("couldn't get records of map ID: {}", map.inner.id))?;

        if map_records.is_empty() {
            continue;
        }

        let mut stats = MapStats {
            records_count: map_records.len() as _,
            min_record: ms_to_sec(map_records[0].time),
            max_record: ms_to_sec(map_records[0].time),
            ..Default::default()
        };

        for record in &map_records {
            stats.min_record = stats.min_record.min(ms_to_sec(record.time));
            stats.max_record = stats.max_record.max(ms_to_sec(record.time));
            stats.average_record += ms_to_sec(record.time);
        }

        stats.average_record /= stats.records_count;
        stats.median_record = ms_to_sec(map_records[map_records.len() / 2].time);

        for (i, record) in map_records.iter().enumerate() {
            let r = (i + 1) as f64;
            let t = ms_to_sec(record.time).max(stats.average_record);
            let score = compute_score(r, stats.records_count, t, stats.average_record);

            let map_score = map_scores.entry(record.map_id).or_insert(0.);
            let player_score = player_scores.entry(record.record_player_id).or_insert(0.);
            *map_score += score;
            *player_score += score;
        }

        map_stats.insert(map.inner.id, stats);
    }

    let mut output = Scores {
        player_scores: player_scores
            .into_iter()
            .map(|(player_id, score)| {
                let player = players.remove(&player_id).unwrap();
                (
                    HashablePlayer {
                        inner: player.inner,
                        unstyled_name: player.unstyled_name,
                    },
                    score,
                )
            })
            .collect(),
        map_scores: map_scores
            .into_iter()
            .map(|(map_id, score)| {
                let map = maps.remove(&map_id).unwrap();
                (
                    HashableMap {
                        inner: map.inner,
                        unstyled_name: map.unstyled_name,
                        stats: map_stats.remove(&map_id).unwrap(),
                    },
                    score,
                )
            })
            .collect(),
    };

    // Insert remaining players and maps, having 0 as score
    for (_, player) in players {
        output.player_scores.insert(
            HashablePlayer {
                inner: player.inner,
                unstyled_name: player.unstyled_name,
            },
            0.,
        );
    }
    for (_, map) in maps {
        output.map_scores.insert(
            HashableMap {
                inner: map.inner,
                unstyled_name: map.unstyled_name,
                stats: Default::default(),
            },
            0.,
        );
    }

    Ok(output)
}
