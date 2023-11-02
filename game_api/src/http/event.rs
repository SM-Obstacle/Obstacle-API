use actix_web::{
    web::{self, Data, Path},
    Responder, Scope,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    auth::{privilege, MPAuthGuard},
    models, must,
    utils::json,
    Database, RecordsResult,
};

use super::{overview, pb, player::UpdatePlayerBody, player_finished as pf};

pub fn event_scope() -> Scope {
    web::scope("/event")
        .service(
            web::scope("/{event_handle}")
                .service(
                    web::scope("/{edition_id}")
                        .route("/overview", web::get().to(edition_overview))
                        .service(
                            web::scope("/player")
                                .route("/finished", web::post().to(edition_finished))
                                .route("/pb", web::get().to(edition_pb)),
                        )
                        .default_service(web::get().to(edition)),
                )
                .default_service(web::get().to(event_editions)),
        )
        .default_service(web::get().to(event_list))
}

pub fn get_sql_fragments() -> (&'static str, &'static str) {
    (
        "INNER JOIN event_edition_records eer ON r.record_id = eer.record_id",
        "AND eer.event_id = ? AND eer.edition_id = ?",
    )
}

pub async fn get_event_by_handle(
    db: &Database,
    handle: &str,
) -> RecordsResult<Option<models::Event>> {
    let r = sqlx::query_as("SELECT * FROM event WHERE handle = ?")
        .bind(handle)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

pub async fn get_edition_by_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Option<models::EventEdition>> {
    let r = sqlx::query_as("SELECT * FROM event_edition WHERE event_id = ? AND id = ?")
        .bind(event_id)
        .bind(edition_id)
        .fetch_optional(&db.mysql_pool)
        .await?;
    Ok(r)
}

pub async fn get_categories_by_edition_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<models::EventCategory>> {
    let r = sqlx::query_as(
        "SELECT DISTINCT ec.* FROM event_edition ee
        INNER JOIN event_edition_categories eec ON eec.edition_id = ee.id
        INNER JOIN event_categories ecs ON ecs.event_id = ee.event_id
        INNER JOIN event_category ec ON ec.id IN (eec.category_id, ecs.category_id)
        WHERE ee.event_id = ? AND ee.id = ?",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_all(&db.mysql_pool)
    .await?;

    Ok(r)
}

pub async fn get_maps_by_edition_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<models::Map>> {
    let r = sqlx::query_as(
        "SELECT *
        FROM maps
        WHERE id IN (
            SELECT map_id
            FROM event_edition_maps
            WHERE event_id = ? AND edition_id = ?
        )",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_all(&db.mysql_pool)
    .await?;

    Ok(r)
}

pub async fn get_maps_by_category_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
    category_id: u32,
) -> RecordsResult<Vec<models::Map>> {
    let r = sqlx::query_as(
        "SELECT *
        FROM maps
        WHERE id IN (
            SELECT map_id
            FROM event_edition_maps
            WHERE event_id = ? AND edition_id = ? AND category_id = ?
        )",
    )
    .bind(event_id)
    .bind(edition_id)
    .bind(category_id)
    .fetch_all(&db.mysql_pool)
    .await?;

    Ok(r)
}

#[derive(Serialize, FromRow)]
pub struct EventResponse {
    handle: String,
    last_edition_id: Option<u32>,
}

#[derive(Serialize, FromRow)]
struct EventHandleResponse {
    id: u32,
    start_date: chrono::NaiveDateTime,
}

#[derive(Serialize)]
struct Map {
    main_author: UpdatePlayerBody,
    other_authors: Vec<String>,
    name: String,
    map_uid: String,
    mx_id: i64,
}

#[derive(Serialize)]
struct Category {
    handle: String,
    name: String,
    banner_img_url: Option<String>,
    maps: Vec<Map>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Content {
    Categories { categories: Vec<Category> },
    Maps { maps: Vec<Map> },
}

#[derive(Serialize)]
struct EventHandleEditionResponse {
    id: u32,
    name: String,
    start_date: chrono::NaiveDateTime,
    banner_img_url: Option<String>,
    content: Content,
}

async fn event_list(db: Data<Database>) -> RecordsResult<impl Responder> {
    let res = sqlx::query_as::<_, EventResponse>(
        "SELECT ev.handle, MAX(ed.id) AS last_edition_id
        FROM event ev
        LEFT JOIN event_edition ed ON ed.event_id = ev.id
        GROUP BY ev.id, ev.handle
        ORDER BY ed.id DESC",
    )
    .fetch_all(&db.mysql_pool)
    .await?;

    json(res)
}

async fn event_editions(
    db: Data<Database>,
    event_handle: Path<String>,
) -> RecordsResult<impl Responder> {
    let event_handle = event_handle.into_inner();
    let id = must::have_event_handle(&db, &event_handle).await?.id;

    let res: Vec<EventHandleResponse> =
        sqlx::query_as("SELECT * FROM event_edition WHERE event_id = ? ORDER BY id DESC")
            .bind(id)
            .fetch_all(&db.mysql_pool)
            .await?;

    json(res)
}

#[derive(Deserialize)]
struct MxMapInfo {
    #[serde(rename = "MapID")]
    map_id: i64,
}

#[derive(Deserialize)]
struct MxAuthor {
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "Uploader")]
    uploader: bool,
}

async fn edition(
    db: Data<Database>,
    client: Data<Client>,
    path: Path<(String, u32)>,
) -> RecordsResult<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();
    let (models::Event { id: event_id, .. }, edition) =
        must::have_event_edition(&db, &event_handle, edition_id).await?;

    let categories = get_categories_by_edition_id(&db, event_id, edition.id).await?;
    let content = if categories.is_empty() {
        let maps = convert_maps(
            &db,
            &client,
            get_maps_by_edition_id(&db, event_id, edition.id).await?,
        )
        .await?;

        Content::Maps { maps }
    } else {
        let mut cat = Vec::with_capacity(categories.len());

        for m in categories {
            let maps = get_maps_by_category_id(&db, event_id, edition.id, m.id).await?;
            cat.push(Category {
                handle: m.handle,
                name: m.name,
                banner_img_url: m.banner_img_url,
                maps: convert_maps(&db, &client, maps).await?,
            });
        }

        Content::Categories { categories: cat }
    };

    json(EventHandleEditionResponse {
        id: edition.id,
        name: edition.name,
        start_date: edition.start_date,
        banner_img_url: edition.banner_img_url,
        content,
    })
}

async fn edition_overview(
    db: Data<Database>,
    path: Path<(String, u32)>,
    query: overview::OverviewReq,
) -> RecordsResult<impl Responder> {
    overview::overview(db, query, Some(path.into_inner())).await
}

async fn edition_finished(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    path: Path<(String, u32)>,
    body: pf::PlayerFinishedBody,
) -> RecordsResult<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    // We first check that the event and its edition exist
    // and that the map is registered on it.
    let event =
        must::have_event_edition_with_map(&db, &body.map_uid, event_handle, edition_id).await?;

    // Then we insert the record for the global records
    let res = pf::finished(login, &db, body, Some(&event)).await?;

    // Then we insert it for the event edition records.
    // This is not part of the transaction for now, because it's not so bad
    // if this query fails.
    let (event, edition) = event;
    sqlx::query(
        "INSERT INTO event_edition_records (record_id, event_id, edition_id)
            VALUES (?, ?, ?)",
    )
    .bind(res.record_id)
    .bind(event.id)
    .bind(edition.id)
    .execute(&db.mysql_pool)
    .await?;

    json(res.res)
}

async fn edition_pb(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    path: Path<(String, u32)>,
    db: Data<Database>,
    body: pb::PbReq,
) -> RecordsResult<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();
    let event =
        must::have_event_edition_with_map(&db, &body.map_uid, event_handle, edition_id).await?;
    pb::pb(login, db, body, Some(event)).await
}

async fn convert_maps(
    db: &Database,
    client: &Client,
    maps: Vec<models::Map>,
) -> RecordsResult<Vec<Map>> {
    const CHUNK_SIZE: usize = 8;

    if maps.is_empty() {
        return Ok(Vec::new());
    }

    let maps_uids = maps
        .iter()
        .map(|m| m.game_id.clone())
        .collect::<Vec<String>>();
    let maps_uids = maps_uids.chunks(CHUNK_SIZE);

    let mut mx_maps_ids = Vec::new();

    for chunk in maps_uids {
        let chunk = chunk.join(",");

        let chunk_ids = client
            .get(format!(
                "https://sm.mania.exchange/api/maps/get_map_info/multi/{chunk}"
            ))
            .header("User-Agent", "obstacle (ahmadbky@5382)")
            .send()
            .await?
            .json::<Vec<MxMapInfo>>()
            .await?
            .into_iter()
            .map(|m| m.map_id);

        mx_maps_ids.extend(chunk_ids);
    }

    let mut out_maps = Vec::with_capacity(maps.len());

    for (m, mx_id) in maps.into_iter().zip(mx_maps_ids) {
        let main_author = sqlx::query_as("SELECT * FROM players WHERE id = ?")
            .bind(m.player_id)
            .fetch_one(&db.mysql_pool)
            .await?;

        let other_authors = client
            .get(format!(
                "https://sm.mania.exchange/api/maps/get_authors/{mx_id}"
            ))
            .header("User-Agent", "obstacle (ahmadbky@5382)")
            .send()
            .await?
            .json::<Vec<MxAuthor>>()
            .await?
            .into_iter()
            .filter_map(|m| (!m.uploader).then_some(m.username))
            .collect();

        out_maps.push(Map {
            main_author,
            other_authors,
            name: m.name,
            map_uid: m.game_id,
            mx_id,
        });
    }

    Ok(out_maps)
}
