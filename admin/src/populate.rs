use std::{collections::HashMap, path::PathBuf};

use anyhow::Context as _;
use deadpool_redis::redis::AsyncCommands as _;
use futures::{stream, StreamExt as _, TryStreamExt};
use itertools::Itertools as _;
use records_lib::{
    event, map,
    mappack::{self, AnyMappackId},
    models, must,
    redis_key::mappack_key,
    Database, DatabaseConnection, MySqlPool,
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

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum Id {
    MapUid { map_uid: String },
    MxId { mx_id: i64 },
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum OriginalId {
    MapUid { original_map_uid: String },
    MxId { original_mx_id: i64 },
}

#[derive(serde::Deserialize, Debug)]
struct Row {
    #[serde(flatten)]
    id: Id,
    category_handle: Option<String>,
    #[serde(flatten)]
    times: Option<MedalTimes>,
    #[serde(flatten)]
    original_id: Option<OriginalId>,
    transitive_save: Option<bool>,
}

#[derive(serde::Deserialize)]
struct MxMapItem {
    #[serde(rename = "TrackID")]
    mx_id: i64,
    #[serde(rename = "AuthorLogin")]
    author_login: String,
    #[serde(rename = "TrackUID")]
    map_uid: String,
    #[serde(rename = "GbxMapName")]
    name: String,
}

async fn insert_mx_maps(
    db: &MySqlPool,
    mx_maps: &[MxMapItem],
) -> anyhow::Result<Vec<(i64, models::Map)>> {
    let mut out = Vec::with_capacity(mx_maps.len());

    let mysql_conn = &mut db.acquire().await?;

    let mut inserted_count = 0;

    for mx_map in mx_maps {
        let author = must::have_player(mysql_conn, &mx_map.author_login).await?;
        // Skip if we already know this map
        if let Some(map) = map::get_map_from_uid(mysql_conn, &mx_map.map_uid).await? {
            out.push((mx_map.mx_id, map));
            continue;
        }

        let id = sqlx::query("insert into maps (game_id, player_id, name) values (?, ?, ?)")
            .bind(&mx_map.map_uid)
            .bind(author.id)
            .bind(&mx_map.name)
            .execute(&mut **mysql_conn)
            .await?
            .last_insert_id();

        let map = sqlx::query_as("select * from maps where id = ?")
            .bind(id)
            .fetch_one(&mut **mysql_conn)
            .await?;

        out.push((mx_map.mx_id, map));
        inserted_count += 1;
    }

    tracing::info!("Inserted {inserted_count} new map(s) from MX");

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
    pool: &MySqlPool,
    rows: &[(Row, u64)],
) -> anyhow::Result<HashMap<i64, models::Map>> {
    tracing::info!("Populating maps from MX...");

    let mx_ids = rows
        .iter()
        .flat_map(|(row, _)| match (&row.id, &row.original_id) {
            (Id::MapUid { .. }, Some(OriginalId::MapUid { .. }) | None) => MxIdIter::None,
            (Id::MxId { mx_id }, Some(OriginalId::MxId { original_mx_id })) => {
                MxIdIter::both(*mx_id, *original_mx_id)
            }
            (Id::MxId { mx_id }, _)
            | (
                _,
                Some(OriginalId::MxId {
                    original_mx_id: mx_id,
                }),
            ) => MxIdIter::single(*mx_id),
        })
        .chunks(25);

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
            insert_mx_maps(pool, &mx_maps).await
        })
        .buffer_unordered(rows.len())
        .try_collect()
        .await?;

    Ok(mx_ids.into_iter().flatten().collect())
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
    let mut conn = db.acquire().await?;

    let (event, edition) =
        must::have_event_edition(&mut conn.mysql_conn, &event_handle, event_edition).await?;

    let categories =
        event::get_categories_by_edition_id(&mut conn.mysql_conn, event.id, edition.id).await?;

    if !categories.is_empty() {
        tracing::info!(
            "Found categories for this event edition: {}",
            categories.iter().map(|cat| &cat.handle).join(", ")
        );
    }

    tracing::info!("Clearing old content...");

    clear::clear_content(&mut conn.mysql_conn, event.id, edition.id).await?;

    let (csv_file, default_transitive_save) = match kind {
        PopulateKind::CsvFile {
            csv_file,
            transitive_save,
        } => (csv_file, transitive_save),
        PopulateKind::MxId { mx_id } => {
            let mx_id = match (mx_id, edition.mx_id) {
                (Some(provided_id), Some(original_id)) if provided_id != original_id => {
                    tracing::warn!("Provided MX id {provided_id} is different from the event edition original MX id {original_id}");
                    provided_id
                }
                (Some(id), _) | (_, Some(id)) => id,
                (None, None) => anyhow::bail!("no MX id provided"),
            };

            let maps =
                map::fetch_mx_mappack_maps(&client, mx_id as _, edition.mx_secret.as_deref())
                    .await?;

            tracing::info!("Found {} map(s) in MX mappack with ID {mx_id}", maps.len());

            for map in maps {
                let player = must::have_player(&mut conn.mysql_conn, &map.AuthorLogin).await?;
                let map_id = match map::get_map_from_uid(&mut conn.mysql_conn, &map.TrackUID)
                    .await?
                {
                    Some(map) => map.id,
                    None => sqlx::query_scalar(
                        "insert into maps (game_id, player_id, name) values (?, ?, ?) returning id",
                    )
                    .bind(&map.TrackUID)
                    .bind(player.id)
                    .bind(&map.GbxMapName)
                    .fetch_one(&mut *conn.mysql_conn)
                    .await?,
                };

                sqlx::query(
                    "REPLACE INTO event_edition_maps (event_id, edition_id, map_id, mx_id, `order`, original_map_id) \
                    VALUES (?, ?, ?, ?, 0, NULL)"
                )
                .bind(event.id)
                .bind(edition.id)
                .bind(map_id)
                .bind(map.MapID)
                .execute(&mut *conn.mysql_conn)
                .await?;
            }

            return Ok(());
        }
    };

    tracing::info!("Collecting CSV rows...");

    let mut reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_path(&csv_file)
        .with_context(|| format!("Couldn't read CSV file `{}`", csv_file.display()))?;

    let mut rows_iter = reader.deserialize::<Row>();
    let mut rows = Vec::with_capacity(rows_iter.size_hint().0);

    loop {
        let line = rows_iter.reader().position().line();
        match rows_iter.next() {
            Some(row) => rows.push((row?, line)),
            None => break,
        }
    }

    conn.mysql_conn.close().await?;

    let mut mx_maps = populate_mx_maps(&client, &db.mysql_pool, &rows).await?;

    let mut conn = DatabaseConnection {
        mysql_conn: db.mysql_pool.acquire().await?,
        ..conn
    };

    tracing::info!("Inserting new content...");

    let mappack = AnyMappackId::Event(&event, &edition);

    for (
        Row {
            id,
            category_handle,
            times,
            original_id,
            transitive_save,
        },
        i,
    ) in rows
    {
        let (mx_id, map) = match id {
            Id::MapUid { map_uid } => (None, must::have_map(&mut conn.mysql_conn, &map_uid).await?),
            Id::MxId { mx_id } => (
                Some(mx_id),
                mx_maps
                    .remove(&mx_id)
                    .ok_or_else(|| anyhow::anyhow!("missing map with mx_id {mx_id}"))
                    .with_context(|| format!("Failed to load the event on line {i}"))?,
            ),
        };

        let (original_mx_id, original_map) = match original_id {
            Some(id) => match id {
                OriginalId::MapUid { original_map_uid } => (
                    None,
                    Some(must::have_map(&mut conn.mysql_conn, &original_map_uid).await?),
                ),
                OriginalId::MxId { original_mx_id } => (
                    Some(original_mx_id),
                    Some(
                        mx_maps
                            .remove(&original_mx_id)
                            .ok_or_else(|| {
                                anyhow::anyhow!("missing map with mx_id {original_mx_id}")
                            })
                            .with_context(|| format!("Failed to load the event on line {i}"))?,
                    ),
                ),
            },
            None => (None, None),
        };

        let opt_category_id = category_handle
            .filter(|s| !s.is_empty())
            .and_then(|s| categories.iter().find(|c| c.handle == s))
            .map(|c| c.id);

        let bronze_time = times.map(|m| m.bronze_time);
        let silver_time = times.map(|m| m.silver_time);
        let gold_time = times.map(|m| m.gold_time);
        let author_time = times.map(|m| m.champion_time);

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
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(opt_category_id)
        .bind(mx_id)
        .bind(i)
        .bind(original_map.as_ref().map(|m| m.id))
        .bind(original_mx_id)
        .bind(transitive_save.unwrap_or(default_transitive_save))
        .bind(bronze_time)
        .bind(silver_time)
        .bind(gold_time)
        .bind(author_time)
        .execute(&mut *conn.mysql_conn)
        .await?;

        conn.redis_conn
            .sadd(mappack_key(mappack), map.game_id)
            .await?;
    }

    tracing::info!("Filling mappack in the Redis database...");

    mappack::update_mappack(mappack, &mut conn).await?;

    tracing::info!("Done");

    Ok(())
}
