use core::fmt;
use std::{collections::HashMap, path::PathBuf};

use anyhow::Context as _;
use deadpool_redis::redis::{self, AsyncCommands as _};
use futures::{StreamExt as _, TryStreamExt, stream};
use itertools::Itertools as _;
use records_lib::{
    Database, DatabaseConnection, MySqlPool, acquire,
    context::{
        Context, Ctx, HasDbPool, HasEdition, HasEditionId, HasEvent, HasEventId, HasMapUid,
        HasPersistentMode, ReadWrite, Transactional,
    },
    event, map,
    mappack::{self, AnyMappackId},
    models, must,
    redis_key::{cached_key, mappack_key},
    time::Time,
    transaction,
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

async fn insert_mx_maps(
    db: MySqlPool,
    mx_maps: &[MxMapItem],
) -> anyhow::Result<Vec<(i64, models::Map)>> {
    let mut out = Vec::with_capacity(mx_maps.len());

    let mut mysql_conn = db.acquire().await?;

    let mut inserted_count = 0;

    let ctx = Context::default();

    for mx_map in mx_maps {
        let ctx = ctx
            .by_ref()
            .with_player_login(&mx_map.author_login)
            .with_map_uid(&mx_map.map_uid);

        let author = must::have_player(&mut mysql_conn, &ctx).await?;
        // Skip if we already know this map
        if let Some(map) = map::get_map_from_uid(&mut mysql_conn, ctx.get_map_uid()).await? {
            out.push((mx_map.mx_id, map));
            continue;
        }

        let id = sqlx::query("insert into maps (game_id, player_id, name) values (?, ?, ?)")
            .bind(&mx_map.map_uid)
            .bind(author.id)
            .bind(&mx_map.name)
            .execute(&mut *mysql_conn)
            .await?
            .last_insert_id();

        let map = sqlx::query_as("select * from maps where id = ?")
            .bind(id)
            .fetch_one(&mut *mysql_conn)
            .await?;

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

#[tracing::instrument(skip(client, pool, rows))]
async fn populate_mx_maps(
    client: &reqwest::Client,
    pool: MySqlPool,
    rows: &[(Row, u64)],
) -> anyhow::Result<HashMap<i64, models::Map>> {
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
        .map(|mut chunk| {
            let pool = pool.clone();
            async move {
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
                insert_mx_maps(pool, &mx_maps).await
            }
        })
        .buffer_unordered(rows.len())
        .try_collect()
        .await?;

    Ok(mx_ids.into_iter().flatten().collect())
}

async fn run_populate<C>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
    client: &reqwest::Client,
    populate_kind: PopulateKind,
) -> anyhow::Result<()>
where
    C: HasEvent + HasEdition + HasPersistentMode + HasDbPool,
{
    let event_key = mappack_key(AnyMappackId::Event(ctx.get_event(), ctx.get_edition()));
    let saved_mappack_key = cached_key(records_lib::gen_random_str(10));

    let key_exists: bool = conn.redis_conn.exists(&event_key).await?;

    if key_exists {
        let _: () = redis::cmd("COPY")
            .arg(&event_key)
            .arg(&saved_mappack_key)
            .exec_async(conn.redis_conn)
            .await?;
    }

    match transaction::within(conn.mysql_conn, &ctx, ReadWrite, async |mysql_conn, ctx| {
        let conn = &mut DatabaseConnection {
            mysql_conn,
            redis_conn: conn.redis_conn,
        };

        tracing::info!("Clearing old content");
        clear::clear_content(conn, ctx.get_event(), ctx.get_edition()).await?;

        match populate_kind {
            PopulateKind::CsvFile {
                csv_file,
                transitive_save,
            } => populate_from_csv(conn, client, ctx, csv_file, transitive_save).await,
            PopulateKind::MxId { mx_id } => populate_from_mx_id(conn, client, ctx, mx_id).await,
        }
    })
    .await
    {
        Ok(_) => {
            // Remove the cached key
            if key_exists {
                let _: () = conn.redis_conn.del(&saved_mappack_key).await?;
            }
            tracing::info!("Success");
            Ok(())
        }
        Err(e) => {
            // Restore the cached key
            if key_exists {
                let _: () = conn
                    .redis_conn
                    .rename(&saved_mappack_key, &event_key)
                    .await?;
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
    let mut conn = acquire!(db?);
    let ctx = Context::default()
        .with_event_handle(&event_handle)
        .with_edition_id(event_edition);

    let (event, edition) = must::have_event_edition(conn.mysql_conn, &ctx).await?;

    let ctx = ctx.with_event(&event).with_edition(&edition).with_pool(db);

    run_populate(&mut conn, &ctx, &client, kind).await?;

    tracing::info!("Filling mappack in the Redis database...");
    mappack::update_mappack(
        Context::default().with_mappack(AnyMappackId::Event(ctx.get_event(), ctx.get_edition())),
        &mut conn,
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

async fn populate_from_csv<C>(
    conn: &mut DatabaseConnection<'_>,
    client: &reqwest::Client,
    ctx: C,
    csv_file: PathBuf,
    default_transitive_save: bool,
) -> anyhow::Result<()>
where
    C: HasEvent + HasEdition + HasDbPool + Transactional<Mode = ReadWrite>,
{
    let mut reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_path(&csv_file)
        .with_context(|| format!("Couldn't read CSV file `{}`", csv_file.display()))?;
    let mut rows_iter = reader.deserialize::<Row>();
    let mut rows = Vec::with_capacity(rows_iter.size_hint().0);

    tracing::info!("Querying event edition categories...");

    let categories = event::get_categories_by_edition_id(
        conn.mysql_conn,
        ctx.get_event_id(),
        ctx.get_edition_id(),
    )
    .await?;

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

    let mx_maps = populate_mx_maps(client, ctx.get_mysql_pool(), &rows).await?;

    for (row, i) in rows {
        let (mx_id, map) = match row.get_id().with_context(|| format!("Parsing row {i}"))? {
            Id::MapUid { map_uid } => (
                None,
                must::have_map(conn.mysql_conn, ctx.by_ref().with_map_uid(map_uid)).await?,
            ),
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
                Id::MapUid { map_uid } => (
                    None,
                    Some(
                        must::have_map(conn.mysql_conn, ctx.by_ref().with_map_uid(map_uid)).await?,
                    ),
                ),
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

        sqlx::query(
            "replace into event_edition_maps (
                event_id,
                edition_id,
                map_id,
                category_id,
                mx_id,
                `order`,
                original_map_id,
                original_mx_id,
                transitive_save,
                bronze_time,
                silver_time,
                gold_time,
                author_time
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(ctx.get_event_id())
        .bind(ctx.get_edition_id())
        .bind(map.id)
        .bind(opt_category_id)
        .bind(mx_id)
        .bind(i)
        .bind(original_map.as_ref().map(|m| m.id))
        .bind(original_mx_id)
        .bind(row.transitive_save.unwrap_or(default_transitive_save))
        .bind(bronze_time)
        .bind(silver_time)
        .bind(gold_time)
        .bind(author_time)
        .execute(&mut **conn.mysql_conn)
        .await?;

        let _: () = conn
            .redis_conn
            .sadd(
                mappack_key(AnyMappackId::Event(ctx.get_event(), ctx.get_edition())),
                map.game_id,
            )
            .await?;
    }

    Ok(())
}

async fn populate_from_mx_id<C>(
    conn: &mut DatabaseConnection<'_>,
    client: &reqwest::Client,
    ctx: C,
    mx_id: Option<i64>,
) -> anyhow::Result<()>
where
    C: HasEvent + HasEdition + Transactional<Mode = ReadWrite>,
{
    let mx_id = match (mx_id, ctx.get_edition().mx_id) {
        (Some(provided_id), Some(original_id)) if provided_id != original_id => {
            tracing::warn!(
                "Provided MX id {provided_id} is different from the event edition original MX id {original_id}"
            );
            provided_id
        }
        (Some(id), _) | (_, Some(id)) => id,
        (None, None) => anyhow::bail!("No MX id provided"),
    };

    let maps =
        map::fetch_mx_mappack_maps(client, mx_id as _, ctx.get_edition().mx_secret.as_deref())
            .await?;

    tracing::info!("Found {} map(s) in MX mappack with ID {mx_id}", maps.len());

    for map in maps {
        let ctx = ctx
            .by_ref()
            .with_player_login(&map.AuthorLogin)
            .with_map_uid(&map.TrackUID);

        let player = must::have_player(conn.mysql_conn, &ctx).await?;
        let map_id = match map::get_map_from_uid(conn.mysql_conn, ctx.get_map_uid()).await? {
            Some(map) => map.id,
            None => {
                sqlx::query_scalar(
                    "insert into maps (game_id, player_id, name) values (?, ?, ?) returning id",
                )
                .bind(&map.TrackUID)
                .bind(player.id)
                .bind(&map.GbxMapName)
                .fetch_one(&mut **conn.mysql_conn)
                .await?
            }
        };

        sqlx::query(
            "REPLACE INTO event_edition_maps (event_id, edition_id, map_id, mx_id, `order`, original_map_id) \
                    VALUES (?, ?, ?, ?, 0, NULL)"
        )
        .bind(ctx.get_event_id())
        .bind(ctx.get_edition_id())
        .bind(map_id)
        .bind(map.MapID)
        .execute(&mut **conn.mysql_conn)
        .await?;
    }

    Ok(())
}
