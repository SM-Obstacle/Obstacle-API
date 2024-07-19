use std::{collections::HashMap, path::PathBuf};

use anyhow::Context as _;
use deadpool_redis::redis::AsyncCommands as _;
use futures::{stream, StreamExt as _, TryStreamExt};
use itertools::Itertools as _;
use records_lib::{
    event, map,
    mappack::{self, AnyMappackId},
    models::{self, Medal},
    must,
    redis_key::mappack_key,
    Database, MySqlPool,
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
    CsvFile { csv_file: PathBuf },
    MxId { mx_id: Option<i64> },
}

#[derive(serde::Deserialize, Debug)]
struct MedalTimes {
    champion_time: i64,
    gold_time: i64,
    silver_time: i64,
    bronze_time: i64,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum Id {
    MapUid { map_uid: String },
    MxId { mx_id: i64 },
}

#[derive(serde::Deserialize, Debug)]
struct Row {
    #[serde(flatten)]
    id: Id,
    category_handle: Option<String>,
    #[serde(flatten)]
    times: Option<MedalTimes>,
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

    for mx_map in mx_maps {
        let author = must::have_player(&mut **mysql_conn, &mx_map.author_login).await?;
        // Skip if we already know this map
        if let Some(map) = map::get_map_from_uid(&mut **mysql_conn, &mx_map.map_uid).await? {
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
    }

    Ok(out)
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
        .filter_map(|(row, _)| match row.id {
            Id::MapUid { .. } => None,
            Id::MxId { mx_id } => Some(mx_id),
        })
        // MX accepts max 50 entries per request
        .chunks(50);

    let mx_ids: Vec<_> = stream::iter(&mx_ids)
        .map(|mut chunk| async move {
            let url = format!(
                "https://sm.mania.exchange/api/maps/get_map_info/multi/{}",
                chunk.join(",")
            );
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
    Database {
        mysql_pool,
        redis_pool,
    }: Database,
    PopulateCommand {
        event_handle,
        event_edition,
        kind,
    }: PopulateCommand,
) -> anyhow::Result<()> {
    let redis_conn = &mut redis_pool.get().await?;
    let mysql_conn = &mut mysql_pool.acquire().await?;

    let (event, edition) =
        must::have_event_edition(mysql_conn, &event_handle, event_edition).await?;

    let categories = event::get_categories_by_edition_id(mysql_conn, event.id, edition.id).await?;

    if !categories.is_empty() {
        tracing::info!(
            "Found categories for this event edition: {}",
            categories.iter().map(|cat| &cat.handle).join(", ")
        );
    }

    tracing::info!("Clearing old content...");

    clear::clear_content(mysql_conn, event.id, edition.id).await?;

    let csv_file = match kind {
        PopulateKind::CsvFile { csv_file } => csv_file,
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
                let player = must::have_player(&mut **mysql_conn, &map.AuthorLogin).await?;
                let map_id = match map::get_map_from_uid(&mut **mysql_conn, &map.TrackUID).await? {
                    Some(map) => map.id,
                    None => sqlx::query_scalar(
                        "insert into maps (game_id, player_id, name) values (?, ?, ?) returning id",
                    )
                    .bind(&map.TrackUID)
                    .bind(player.id)
                    .bind(&map.GbxMapName)
                    .fetch_one(&mut **mysql_conn)
                    .await?,
                };

                sqlx::query(
                    "REPLACE INTO event_edition_maps (event_id, edition_id, map_id, mx_id, `order`) \
                    VALUES (?, ?, ?, ?, 0)"
                )
                .bind(event.id)
                .bind(edition.id)
                .bind(map_id)
                .bind(map.MapID)
                .execute(&mut **mysql_conn)
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

    let mut rows = Vec::new();
    let mut rows_iter = reader.deserialize::<Row>();

    loop {
        let line = rows_iter.reader().position().line();
        match rows_iter.next() {
            Some(row) => rows.push((row?, line)),
            None => break,
        }
    }

    let mut mx_maps = populate_mx_maps(&client, &mysql_pool, &rows).await?;

    tracing::info!("Retrieved these MX maps: {mx_maps:#?}");

    tracing::info!("Inserting new content...");

    let mappack = AnyMappackId::Event(&event, &edition);

    for (
        Row {
            id,
            category_handle,
            times,
        },
        i,
    ) in rows
    {
        let (mx_id, map) = match id {
            Id::MapUid { map_uid } => (0, must::have_map(&mut **mysql_conn, &map_uid).await?),
            Id::MxId { mx_id } => (
                mx_id,
                mx_maps
                    .remove(&mx_id)
                    .ok_or_else(|| anyhow::anyhow!("missing map with mx_id {mx_id}"))
                    .with_context(|| format!("Failed to load the event on line {i}"))?,
            ),
        };

        let opt_category_id = category_handle
            .filter(|s| !s.is_empty())
            .and_then(|s| categories.iter().find(|c| c.handle == s))
            .map(|c| c.id);

        sqlx::query(
            "replace into event_edition_maps (event_id, edition_id, map_id, category_id, mx_id, `order`) \
        values (?, ?, ?, ?, ?, ?)",
        )
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(opt_category_id)
        .bind(mx_id)
        .bind(i)
        .execute(&mut **mysql_conn)
        .await?;

        let Some(MedalTimes {
            bronze_time,
            silver_time,
            gold_time,
            champion_time,
        }) = times
        else {
            continue;
        };

        sqlx::query(
            "replace into event_edition_maps_medals
            (event_id, edition_id, map_id, medal_id, time)
            values (?, ?, ?, ?, ?),
                   (?, ?, ?, ?, ?),
                   (?, ?, ?, ?, ?),
                   (?, ?, ?, ?, ?)",
        )
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(Medal::Bronze as u8)
        .bind(bronze_time)
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(Medal::Silver as u8)
        .bind(silver_time)
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(Medal::Gold as u8)
        .bind(gold_time)
        .bind(event.id)
        .bind(edition.id)
        .bind(map.id)
        .bind(Medal::Champion as u8)
        .bind(champion_time)
        .execute(&mut **mysql_conn)
        .await?;

        redis_conn.sadd(mappack_key(mappack), map.game_id).await?;
    }

    tracing::info!("Filling mappack in the Redis database...");

    mappack::update_mappack(mappack, mysql_conn, redis_conn).await?;

    tracing::info!("Done");

    Ok(())
}
