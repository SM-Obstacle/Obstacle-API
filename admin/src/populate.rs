use std::{collections::HashMap, path::PathBuf};

use anyhow::Context as _;
use futures::{stream, StreamExt as _, TryStreamExt};
use itertools::Itertools as _;
use records_lib::{
    event, map,
    models::{self, Medal},
    must, MySqlPool,
};

use crate::clear;

#[derive(clap::Args, Debug)]
pub struct PopulateCommand {
    event_handle: String,
    event_edition: u32,
    csv_file: PathBuf,
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
        if let Some(map) = map::get_map_from_game_id(&mut **mysql_conn, &mx_map.map_uid).await? {
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
            insert_mx_maps(&pool, &mx_maps).await
        })
        .buffer_unordered(rows.len())
        .try_collect()
        .await?;

    Ok(mx_ids.into_iter().flatten().collect())
}

pub async fn populate(
    client: reqwest::Client,
    db: MySqlPool,
    PopulateCommand {
        event_handle,
        event_edition,
        csv_file,
    }: PopulateCommand,
) -> anyhow::Result<()> {
    let mysql_conn = &mut db.acquire().await?;

    let (event, edition) =
        must::have_event_edition(mysql_conn, &event_handle, event_edition).await?;

    let categories =
        event::get_categories_by_edition_id(&mut **mysql_conn, event.id, edition.id).await?;

    tracing::info!(
        "Found categories for this event edition: {}",
        categories.iter().map(|cat| &cat.handle).join(", ")
    );

    tracing::info!("Clearing old content...");

    clear::clear_content(mysql_conn, event.id, edition.id).await?;

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

    let mut mx_maps = populate_mx_maps(&client, &db, &rows).await?;

    tracing::info!("Retrieved these MX maps: {mx_maps:#?}");

    tracing::info!("Inserting new content...");

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

        // FIXME: I'm too lazy to make a single request
        for (medal_id, time) in [
            (Medal::Bronze as u8, bronze_time),
            (Medal::Silver as _, silver_time),
            (Medal::Gold as _, gold_time),
            (Medal::Champion as _, champion_time),
        ] {
            sqlx::query("replace into event_edition_maps_medals (event_id, edition_id, map_id, medal_id, time) \
            values (?, ?, ?, ?, ?)")
                .bind(event.id)
                .bind(edition.id)
                .bind(map.id)
                .bind(medal_id)
                .bind(time).execute(&mut **mysql_conn).await?;
        }
    }

    tracing::info!("Done");

    Ok(())
}
