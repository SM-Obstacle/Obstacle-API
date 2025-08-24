use core::fmt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use deadpool_redis::redis::{self, AsyncCommands as _};
use entity::{event, event_edition, event_edition_maps, maps};
use futures::{StreamExt as _, TryStreamExt, stream};
use itertools::Itertools as _;
use records_lib::{
    Database, RedisConnection, map,
    mappack::{self, AnyMappackId},
    must,
    redis_key::{cached_key, mappack_key},
    time::Time,
    transaction,
};
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, EntityTrait, StatementBuilder, TransactionTrait,
    sea_query::Query,
};

use crate::clear;

#[derive(clap::Args, Debug)]
pub struct PopulateCommand {
    event_handle: String,
    event_edition: u32,
    #[clap(subcommand)]
    kind: PopulateKind,
}

#[derive(clap::Subcommand, Debug)]
enum PopulateKind {
    CsvFile {
        csv_file: PathBuf,
        #[clap(long)]
        #[clap(default_value_t = true)]
        transitive_save: bool,
    },
    MxId {
        mx_id: Option<i64>,
    },
}

#[derive(serde::Deserialize, Debug, Clone, Copy)]
struct MedalTimes {
    champion_time: i32,
    gold_time: i32,
    silver_time: i32,
    bronze_time: i32,
}

#[derive(Debug)]
enum Id<'a> {
    MapUid { map_uid: &'a str },
    MxId { mx_id: i64 },
}

#[derive(serde::Deserialize, Debug)]
struct Row {
    map_uid: Option<String>,
    mx_id: Option<i64>,
    category_handle: Option<String>,
    #[serde(flatten)]
    times: Option<MedalTimes>,
    original_map_uid: Option<String>,
    original_mx_id: Option<i64>,
    transitive_save: Option<bool>,
}

fn get_id_impl(uid: Option<&str>, mx_id: Option<i64>) -> Option<Id<'_>> {
    uid.map(|map_uid| Id::MapUid { map_uid })
        .or(mx_id.map(|mx_id| Id::MxId { mx_id }))
}

impl Row {
    fn get_id(&self) -> anyhow::Result<Id<'_>> {
        get_id_impl(self.map_uid.as_deref(), self.mx_id)
            .ok_or_else(|| anyhow::anyhow!("You must provide either the map UID or the MX ID"))
    }

    fn get_original_id(&self) -> Option<Id<'_>> {
        get_id_impl(self.original_map_uid.as_deref(), self.original_mx_id)
    }
}

#[derive(serde::Deserialize)]
struct MxMapItem {
    #[serde(rename = "MapID")]
    mx_id: i64,
    #[serde(rename = "AuthorLogin")]
    author_login: String,
    #[serde(rename = "TrackUID")]
    map_uid: String,
    #[serde(rename = "GbxMapName")]
    name: String,
}

async fn insert_mx_maps<C: ConnectionTrait>(
    conn: &C,
    mx_maps: &[MxMapItem],
) -> anyhow::Result<Vec<(i64, maps::Model)>> {
    let mut out = Vec::with_capacity(mx_maps.len());

    let mut inserted_count = 0;

    for mx_map in mx_maps {
        let author = must::have_player(conn, &mx_map.author_login).await?;
        // Skip if we already know this map
        if let Some(map) = map::get_map_from_uid(conn, &mx_map.map_uid).await? {
            out.push((mx_map.mx_id, map));
            continue;
        }

        let map = maps::ActiveModel {
            game_id: Set(mx_map.map_uid.clone()),
            player_id: Set(author.id),
            name: Set(mx_map.name.clone()),
            ..Default::default()
        };
        let map = maps::Entity::insert(map).exec_with_returning(conn).await?;

        out.push((mx_map.mx_id, map));
        inserted_count += 1;
    }

    if inserted_count > 0 {
        tracing::info!("Inserted {inserted_count} new map(s) from MX");
    }

    Ok(out)
}

enum MxIdIter {
    None,
    Single(Option<i64>),
    Both(Option<i64>, Option<i64>),
}

impl MxIdIter {
    #[inline]
    fn single(x: i64) -> Self {
        Self::Single(Some(x))
    }

    #[inline]
    fn both(a: i64, b: i64) -> Self {
        Self::Both(Some(a), Some(b))
    }
}

impl Iterator for MxIdIter {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::None => None,
            Self::Single(x) => x.take(),
            Self::Both(first, second) => first.take().or_else(|| second.take()),
        }
    }
}

#[tracing::instrument(skip(client, conn, rows))]
async fn populate_mx_maps<C: ConnectionTrait>(
    client: &reqwest::Client,
    conn: &C,
    rows: &[(Row, u64)],
) -> anyhow::Result<HashMap<i64, maps::Model>> {
    tracing::info!("Populating maps from MX...");

    let mx_ids = rows
        .iter()
        .flat_map(|(row, _)| match (row.get_id(), row.get_original_id()) {
            (Ok(Id::MapUid { .. }) | Err(_), Some(Id::MapUid { .. }) | None) => MxIdIter::None,
            (
                Ok(Id::MxId { mx_id }),
                Some(Id::MxId {
                    mx_id: original_mx_id,
                }),
            ) => MxIdIter::both(mx_id, original_mx_id),
            (Ok(Id::MxId { mx_id }), _) | (_, Some(Id::MxId { mx_id })) => MxIdIter::single(mx_id),
        })
        .chunks(10);

    let mx_ids: Vec<_> = stream::iter(&mx_ids)
        .map(|mut chunk| async move {
            let url = format!(
                "https://sm.mania.exchange/api/maps/get_map_info/multi/{}",
                chunk.join(",")
            );
            tracing::info!("Requesting MX ({})...", url);
            let mx_maps = client
                .get(url)
                .header("User-Agent", "obstacle (discord @ahmadbky)")
                .send()
                .await?
                .json::<Vec<MxMapItem>>()
                .await?;
            insert_mx_maps(conn, &mx_maps).await
        })
        .buffer_unordered(rows.len())
        .try_collect()
        .await?;

    Ok(mx_ids.into_iter().flatten().collect())
}

async fn run_populate<C: TransactionTrait + ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    event: &event::Model,
    edition: &event_edition::Model,
    client: &reqwest::Client,
    populate_kind: PopulateKind,
) -> anyhow::Result<()> {
    let event_key = mappack_key(AnyMappackId::Event(event, edition));
    let saved_mappack_key = cached_key(records_lib::gen_random_str(10));

    let key_exists: bool = redis_conn.exists(&event_key).await?;

    if key_exists {
        let _: () = redis::cmd("COPY")
            .arg(&event_key)
            .arg(&saved_mappack_key)
            .exec_async(redis_conn)
            .await?;
    }

    match transaction::within(conn, async |txn| {
        tracing::info!("Clearing old content");
        clear::clear_content(txn, redis_conn, event, edition).await?;

        match populate_kind {
            PopulateKind::CsvFile {
                csv_file,
                transitive_save,
            } => {
                populate_from_csv(
                    conn,
                    redis_conn,
                    client,
                    (event, edition),
                    &csv_file,
                    transitive_save,
                )
                .await
            }
            PopulateKind::MxId { mx_id } => {
                populate_from_mx_id(conn, client, event, edition, mx_id).await
            }
        }
    })
    .await
    {
        Ok(_) => {
            // Remove the cached key
            if key_exists {
                let _: () = redis_conn.del(&saved_mappack_key).await?;
            }
            tracing::info!("Success");
            Ok(())
        }
        Err(e) => {
            // Restore the cached key
            if key_exists {
                let _: () = redis_conn.rename(&saved_mappack_key, &event_key).await?;
            }
            tracing::info!("Operation failed, restored old content");
            Err(e)
        }
    }
}

pub async fn populate(
    client: reqwest::Client,
    db: Database,
    PopulateCommand {
        event_handle,
        event_edition,
        kind,
    }: PopulateCommand,
) -> anyhow::Result<()> {
    let mut redis_conn = db.redis_pool.get().await?;

    let (event, edition) =
        must::have_event_edition(&db.sql_conn, &event_handle, event_edition).await?;

    run_populate(
        &db.sql_conn,
        &mut redis_conn,
        &event,
        &edition,
        &client,
        kind,
    )
    .await?;

    tracing::info!("Filling mappack in the Redis database...");
    mappack::update_mappack(
        &db.sql_conn,
        &mut redis_conn,
        AnyMappackId::Event(&event, &edition),
        Default::default(),
    )
    .await?;

    Ok(())
}

struct CsvReadErr {
    line: u64,
}

impl From<u64> for CsvReadErr {
    #[inline(always)]
    fn from(line: u64) -> Self {
        Self { line }
    }
}

impl fmt::Display for CsvReadErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to read the CSV on line {}", self.line)
    }
}

fn check_inconsistent_medal_times(
    line: u64,
    (medal_time, medal_label): (i32, &str),
    (next_medal_time, next_medal_label): (i32, &str),
) {
    if medal_time < next_medal_time {
        tracing::warn!(
            "Inconsistent medal times on line {line}: {medal_label} time is lower than {next_medal_label} time;\n\
            {} < {} ({medal_time} < {next_medal_time})",
            Time(medal_time),
            Time(next_medal_time)
        );
    }
}

fn check_medal_times_consistency(
    line: u64,
    bronze_time: Option<i32>,
    silver_time: Option<i32>,
    gold_time: Option<i32>,
    author_time: Option<i32>,
) {
    let (Some(bronze_time), Some(silver_time), Some(gold_time), Some(author_time)) =
        (bronze_time, silver_time, gold_time, author_time)
    else {
        return;
    };

    let bronze_span = (bronze_time, "bronze");
    let silver_span = (silver_time, "silver");
    let gold_span = (gold_time, "gold");
    let author_span = (author_time, "champion");

    check_inconsistent_medal_times(line, bronze_span, silver_span);
    check_inconsistent_medal_times(line, silver_span, gold_span);
    check_inconsistent_medal_times(line, gold_span, author_span);
}

async fn populate_from_csv<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    client: &reqwest::Client,
    (event, edition): (&event::Model, &event_edition::Model),
    csv_file: &Path,
    default_transitive_save: bool,
) -> anyhow::Result<()> {
    let mut reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_path(csv_file)
        .with_context(|| format!("Couldn't read CSV file `{}`", csv_file.display()))?;
    let mut rows_iter = reader.deserialize::<Row>();
    let mut rows = Vec::with_capacity(rows_iter.size_hint().0);

    tracing::info!("Querying event edition categories...");

    let categories =
        records_lib::event::get_categories_by_edition_id(conn, event.id, edition.id).await?;

    if categories.is_empty() {
        tracing::info!("No category found for this event edition");
    } else {
        tracing::info!(
            "Found categories for this event edition: {}",
            categories.iter().map(|cat| &cat.handle).join(", ")
        );
    }

    tracing::info!("Collecting CSV rows...");

    loop {
        let line = rows_iter.reader().position().line();
        match rows_iter.next() {
            Some(row) => {
                let row = row?;
                if row.category_handle.is_none() && !categories.is_empty() {
                    tracing::warn!("Missing map category at line: {line}");
                }
                rows.push((row, line));
            }
            None => break,
        }
    }

    tracing::info!("Inserting new content...");

    let mx_maps = populate_mx_maps(client, conn, &rows).await?;

    for (row, i) in rows {
        let (mx_id, map) = match row.get_id().with_context(|| format!("Parsing row {i}"))? {
            Id::MapUid { map_uid } => (None, must::have_map(conn, map_uid).await?),
            Id::MxId { mx_id } => (
                Some(mx_id),
                mx_maps
                    .get(&mx_id)
                    .ok_or_else(|| anyhow::anyhow!("Missing map with MX ID {mx_id}"))
                    .with_context(|| CsvReadErr::from(i))?
                    .clone(),
            ),
        };

        let (original_mx_id, original_map) = match row.get_original_id() {
            Some(id) => match id {
                Id::MapUid { map_uid } => (None, Some(must::have_map(conn, map_uid).await?)),
                Id::MxId { mx_id } => (
                    Some(mx_id),
                    Some(
                        mx_maps
                            .get(&mx_id)
                            .ok_or_else(|| anyhow::anyhow!("Missing map with MX ID {mx_id}"))
                            .with_context(|| CsvReadErr::from(i))?
                            .clone(),
                    ),
                ),
            },
            None => (None, None),
        };

        let opt_category_id = row
            .category_handle
            .filter(|s| !s.is_empty())
            .and_then(|s| categories.iter().find(|c| c.handle == s))
            .map(|c| c.id);

        let bronze_time = row.times.map(|m| m.bronze_time);
        let silver_time = row.times.map(|m| m.silver_time);
        let gold_time = row.times.map(|m| m.gold_time);
        let author_time = row.times.map(|m| m.champion_time);

        check_medal_times_consistency(i, bronze_time, silver_time, gold_time, author_time);

        let mut replace = Query::insert();
        let replace = replace
            .replace()
            .columns([
                event_edition_maps::Column::EventId,
                event_edition_maps::Column::EditionId,
                event_edition_maps::Column::MapId,
                event_edition_maps::Column::CategoryId,
                event_edition_maps::Column::MxId,
                event_edition_maps::Column::Order,
                event_edition_maps::Column::OriginalMapId,
                event_edition_maps::Column::OriginalMxId,
                event_edition_maps::Column::TransitiveSave,
                event_edition_maps::Column::BronzeTime,
                event_edition_maps::Column::SilverTime,
                event_edition_maps::Column::GoldTime,
                event_edition_maps::Column::AuthorTime,
            ])
            .values_panic([
                event.id.into(),
                edition.id.into(),
                map.id.into(),
                opt_category_id.into(),
                mx_id.into(),
                i.into(),
                original_map.as_ref().map(|m| m.id).into(),
                original_mx_id.into(),
                row.transitive_save
                    .unwrap_or(default_transitive_save)
                    .into(),
                bronze_time.into(),
                silver_time.into(),
                gold_time.into(),
                author_time.into(),
            ]);
        let stmt = StatementBuilder::build(&*replace, &conn.get_database_backend());
        conn.execute(stmt).await?;

        let _: () = redis_conn
            .sadd(
                mappack_key(AnyMappackId::Event(event, edition)),
                map.game_id,
            )
            .await?;
    }

    Ok(())
}

async fn populate_from_mx_id<C: ConnectionTrait>(
    conn: &C,
    client: &reqwest::Client,
    event: &event::Model,
    edition: &event_edition::Model,
    mx_id: Option<i64>,
) -> anyhow::Result<()> {
    let mx_id = match (mx_id, edition.mx_id.map(|x| x as i64)) {
        (Some(provided_id), Some(original_id)) if provided_id != original_id => {
            tracing::warn!(
                "Provided MX id {provided_id} is different from the event edition original MX id {original_id}"
            );
            provided_id
        }
        (Some(id), _) | (_, Some(id)) => id,
        (None, None) => anyhow::bail!("No MX id provided"),
    };

    let maps = map::fetch_mx_mappack_maps(client, mx_id as _, edition.mx_secret.as_deref()).await?;

    tracing::info!("Found {} map(s) in MX mappack with ID {mx_id}", maps.len());

    for map in maps {
        let player = must::have_player(conn, &map.AuthorLogin).await?;

        let map_id = match map::get_map_from_uid(conn, &map.TrackUID).await? {
            Some(map) => map.id,
            None => {
                let map = maps::ActiveModel {
                    game_id: Set(map.TrackUID),
                    player_id: Set(player.id),
                    name: Set(map.GbxMapName),
                    ..Default::default()
                };
                maps::Entity::insert(map)
                    .exec_with_returning_keys(conn)
                    .await?[0]
            }
        };

        let mut replace = Query::insert();
        let replace = replace
            .replace()
            .into_table(event_edition_maps::Entity)
            .columns([
                event_edition_maps::Column::EventId,
                event_edition_maps::Column::EditionId,
                event_edition_maps::Column::MapId,
                event_edition_maps::Column::MxId,
                event_edition_maps::Column::Order,
                event_edition_maps::Column::OriginalMapId,
            ])
            .values_panic([
                event.id.into(),
                edition.id.into(),
                map_id.into(),
                map.MapID.into(),
                0.into(),
                None::<u32>.into(),
            ]);
        let stmt = StatementBuilder::build(&*replace, &conn.get_database_backend());
        conn.execute(stmt).await?;
    }

    Ok(())
}
