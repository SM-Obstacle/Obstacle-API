use actix_web::{
    web::{self, Path},
    Responder, Scope,
};
use futures::TryStreamExt;
use itertools::Itertools;
use records_lib::{
    error::RecordsError,
    event::{self, OptEvent},
    models, opt_ser, Database, MpDefaultI32,
};
use serde::Serialize;
use sqlx::{FromRow, MySqlConnection};
use tracing_actix_web::RequestId;

use crate::{
    auth::MPAuthGuard, utils::json, FitRequestId, RecordsErrorKind, RecordsResponse, RecordsResult,
    RecordsResultExt, Res,
};

use super::{overview, pb, player::PlayerInfoNetBody, player_finished as pf};

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

#[derive(FromRow)]
struct MapWithCategory {
    #[sqlx(flatten)]
    map: models::Map,
    category_id: Option<u32>,
    mx_id: i64,
}

async fn get_maps_by_edition_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<MapWithCategory>> {
    let r = sqlx::query_as(
        "SELECT m.*, category_id, mx_id
        FROM maps m
        INNER JOIN event_edition_maps eem ON id = map_id
        AND event_id = ? AND edition_id = ?
        ORDER BY category_id, eem.order",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_all(&db.mysql_pool)
    .await
    .with_api_err()?;

    Ok(r)
}

#[derive(Serialize, FromRow)]
struct RawEventHandleResponse {
    id: u32,
    name: String,
    #[serde(skip)]
    subtitle: Option<String>,
    start_date: chrono::NaiveDateTime,
}

#[derive(Serialize)]
struct EventHandleResponse {
    subtitle: String,
    #[serde(flatten)]
    raw: RawEventHandleResponse,
}

#[derive(Serialize)]
struct Map {
    mx_id: i64,
    main_author: PlayerInfoNetBody,
    name: String,
    map_uid: String,
    bronze_time: i32,
    silver_time: i32,
    gold_time: i32,
    champion_time: i32,
    #[serde(serialize_with = "opt_ser")]
    personal_best: Option<MpDefaultI32>,
    next_opponent: NextOpponent,
}

#[derive(Serialize, Default)]
struct Category {
    handle: String,
    name: String,
    banner_img_url: String,
    maps: Vec<Map>,
}

impl From<Vec<Map>> for Category {
    fn from(maps: Vec<Map>) -> Self {
        Self {
            maps,
            ..Default::default()
        }
    }
}

/// An event edition from the point of view of the Obstacle gamemode.
#[derive(Serialize)]
struct EventHandleEditionResponse {
    /// The edition ID.
    id: u32,
    /// The name of the edition.
    name: String,
    /// The subtitle of the edition.
    ///
    /// For the 2nd campaign, the name is "Winter" and the subtitle is "2024" for example.
    /// It is empty if it doesn't have a subtitle.
    subtitle: String,
    /// The ManiaPlanet nicknames of the authors.
    authors: Vec<String>,
    /// The formatted start date of the edition, in the format dd/mm/YYYY HH:MM.
    start_date: String,
    /// The formatted end date of the edition, in the format dd/mm/YYYY HH:MM.
    ///
    /// This is empty if the edition doesn't expire.
    end_date: String,
    /// The URL to the banner image of the edition, used in the Titlepack menu.
    banner_img_url: String,
    /// The URL to the small banner image of the edition, used in game in the Campaign mode.
    banner2_img_url: String,
    /// The MX ID of the related mappack.
    #[serde(serialize_with = "opt_ser")]
    mx_id: Option<MpDefaultI32>,
    /// Whether the edition has expired or not.
    expired: bool,
    /// The content of the edition (the categories).
    ///
    /// If the event has no category, the maps are grouped in a single category with all
    /// its fields empty.
    categories: Vec<Category>,
}

async fn event_list(req_id: RequestId, db: Res<Database>) -> RecordsResponse<impl Responder> {
    let mysql_conn = &mut db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let out = event::event_list(mysql_conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(out)
}

async fn event_editions(
    db: Res<Database>,
    req_id: RequestId,
    event_handle: Path<String>,
) -> RecordsResponse<impl Responder> {
    let event_handle = event_handle.into_inner();

    let mysql_conn = &mut db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let id = records_lib::must::have_event_handle(mysql_conn, &event_handle)
        .await
        .fit(req_id)?
        .id;

    let res: Vec<RawEventHandleResponse> = sqlx::query_as(
        "select ee.* from event_edition ee
        where ee.event_id = ? and ee.start_date < sysdate()
            and (ee.event_id, ee.id) in (
            select eem.event_id, eem.edition_id from event_edition_maps eem
        ) order by ee.id desc",
    )
    .bind(id)
    .fetch_all(&mut **mysql_conn)
    .await
    .with_api_err()
    .fit(req_id)?;

    json(
        res.into_iter()
            .map(|raw| EventHandleResponse {
                subtitle: raw.subtitle.clone().unwrap_or_default(),
                raw,
            })
            .collect_vec(),
    )
}

#[derive(FromRow, Serialize, Default)]
struct NextOpponent {
    #[serde(serialize_with = "opt_ser")]
    login: Option<String>,
    #[serde(serialize_with = "opt_ser")]
    name: Option<String>,
    #[serde(serialize_with = "opt_ser")]
    time: Option<MpDefaultI32>,
}

struct AuthorWithPlayerTime {
    /// The author of the map
    main_author: PlayerInfoNetBody,
    /// The time of the player (not the same player as the author)
    personal_best: Option<MpDefaultI32>,
    /// The next opponent of the player
    next_opponent: Option<NextOpponent>,
}

async fn edition(
    auth: Option<MPAuthGuard>,
    db: Res<Database>,
    req_id: RequestId,
    path: Path<(String, u32)>,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    let mysql_conn = &mut db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let (models::Event { id: event_id, .. }, edition) =
        records_lib::must::have_event_edition(mysql_conn, &event_handle, edition_id)
            .await
            .fit(req_id)?;

    // The edition is not yet released
    if chrono::Utc::now() < edition.start_date.and_utc() {
        return Err(RecordsError::EventEditionNotFound(event_handle, edition_id))
            .with_api_err()
            .fit(req_id);
    }

    let maps = get_maps_by_edition_id(&db, event_id, edition_id)
        .await
        .fit(req_id)?
        .into_iter()
        .chunk_by(|m| m.category_id);
    let maps = maps.into_iter();

    let mysql_conn = &mut db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;
    let mut cat = event::get_categories_by_edition_id(mysql_conn, event_id, edition.id)
        .await
        .fit(req_id)?;

    let mut categories = Vec::with_capacity(cat.len());

    for (cat_id, cat_maps) in maps {
        let m = cat_id
            .and_then(|c_id| cat.iter().find_position(|c| c.id == c_id))
            .map(|(i, _)| i)
            .map(|i| cat.swap_remove(i))
            .unwrap_or_default();

        let mut maps = Vec::with_capacity(cat_maps.size_hint().0);

        for MapWithCategory { map, mx_id, .. } in cat_maps {
            let AuthorWithPlayerTime {
                main_author,
                personal_best,
                next_opponent,
            } = if let Some(MPAuthGuard { login }) = &auth {
                let main_author = sqlx::query_as("select * from players where id = ?")
                    .bind(map.player_id)
                    .fetch_one(&db.mysql_pool)
                    .await
                    .with_api_err()
                    .fit(req_id)?;

                let personal_best = sqlx::query_scalar(
                    "select min(time) from records r
                    inner join players p on p.id = r.record_player_id
                    inner join event_edition_records eer on eer.record_id = r.record_id
                        and eer.event_id = ? and eer.edition_id = ?
                    where p.login = ? and r.map_id = ?",
                )
                .bind(event_id)
                .bind(edition_id)
                .bind(login)
                .bind(map.id)
                .fetch_one(&db.mysql_pool)
                .await
                .with_api_err()
                .fit(req_id)?;

                let next_opponent = sqlx::query_as(
                    "select p.login, p.name, gr2.time from global_event_records gr
                    inner join players player_from
                    on player_from.id = gr.record_player_id
                    inner join global_event_records gr2
                    on gr2.map_id = gr.map_id
                        and gr2.event_id = gr.event_id
                        and gr2.edition_id = gr.edition_id
                        and gr2.time < gr.time
                    inner join players p on p.id = gr2.record_player_id
                    where player_from.login = ?
                        and gr.map_id = ?
                        and gr.event_id = ?
                        and gr.edition_id = ?
                    order by gr2.time desc
                    limit 1",
                )
                .bind(login)
                .bind(map.id)
                .bind(event_id)
                .bind(edition_id)
                .fetch_optional(&db.mysql_pool)
                .await
                .with_api_err()
                .fit(req_id)?;

                AuthorWithPlayerTime {
                    main_author,
                    personal_best,
                    next_opponent,
                }
            } else {
                AuthorWithPlayerTime {
                    main_author: sqlx::query_as("select * from players where id = ?")
                        .bind(map.player_id)
                        .fetch_one(&db.mysql_pool)
                        .await
                        .with_api_err()
                        .fit(req_id)?,
                    personal_best: None,
                    next_opponent: None,
                }
            };

            let medal_times =
                event::get_medal_times_of(&db.mysql_pool, event_id, edition_id, map.id)
                    .await
                    .with_api_err()
                    .fit(req_id)?;

            maps.push(Map {
                mx_id,
                main_author,
                name: map.name,
                map_uid: map.game_id,
                bronze_time: medal_times.bronze_time,
                silver_time: medal_times.silver_time,
                gold_time: medal_times.gold_time,
                champion_time: medal_times.champion_time,
                personal_best,
                next_opponent: next_opponent.unwrap_or_default(),
            });
        }

        categories.push(Category {
            handle: m.handle,
            name: m.name,
            banner_img_url: m.banner_img_url.unwrap_or_default(),
            maps,
        });
    }

    // Fill with empty categories
    for cat in cat {
        categories.push(Category {
            handle: cat.handle,
            name: cat.name,
            banner_img_url: cat.banner_img_url.unwrap_or_default(),
            maps: Vec::new(),
        });
    }

    json(EventHandleEditionResponse {
        expired: edition.has_expired(),
        end_date: edition
            .expire_date()
            .map(|d| d.format("%d/%m/%Y %H:%M").to_string())
            .unwrap_or_default(),
        id: edition.id,
        name: edition.name,
        subtitle: edition.subtitle.unwrap_or_default(),
        authors: event::get_admins_of(mysql_conn, edition.event_id, edition.id)
            .map_ok(|p| p.name)
            .try_collect()
            .await
            .with_api_err()
            .fit(req_id)?,
        start_date: edition.start_date.format("%d/%m/%Y %H:%M").to_string(),
        banner_img_url: edition.banner_img_url.unwrap_or_default(),
        banner2_img_url: edition.banner2_img_url.unwrap_or_default(),
        mx_id: edition.mx_id.map(|id| MpDefaultI32(id as _)),
        categories,
    })
}

async fn edition_overview(
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    query: overview::OverviewReq,
) -> RecordsResponse<impl Responder> {
    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;
    let (event, edition) = path.into_inner();
    let (event, edition) = records_lib::must::have_event_edition_with_map(
        &mut mysql_conn,
        &query.map_uid,
        event,
        edition,
    )
    .await
    .with_api_err()
    .fit(req_id)?;
    mysql_conn.close().await.with_api_err().fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    overview::overview(req_id, db, query, OptEvent::new(&event, &edition)).await
}

#[inline(always)]
async fn edition_finished(
    MPAuthGuard { login }: MPAuthGuard,
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    body: pf::PlayerFinishedBody,
) -> RecordsResponse<impl Responder> {
    edition_finished_at(
        login,
        req_id,
        db,
        path,
        body.0,
        chrono::Utc::now().naive_utc(),
    )
    .await
}

pub async fn edition_finished_at(
    login: String,
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    // We first check that the event and its edition exist
    // and that the map is registered on it.
    let (event, edition) = records_lib::must::have_event_edition_with_map(
        &mut mysql_conn,
        &body.map_uid,
        event_handle,
        edition_id,
    )
    .await
    .fit(req_id)?;

    mysql_conn.close().await.with_api_err().fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    // Then we insert the record for the global records
    let res = pf::finished(login, &db, body, OptEvent::new(&event, &edition), at)
        .await
        .fit(req_id)?;

    // Then we insert it for the event edition records.
    // This is not part of the transaction, because we don't want to roll back
    // the insertion of the record if this query fails.
    insert_event_record(
        &mut *db.mysql_pool.acquire().await.with_api_err().fit(req_id)?,
        res.record_id,
        event.id,
        edition.id,
    )
    .await
    .fit(req_id)?;

    json(res.res)
}

pub async fn insert_event_record(
    conn: &mut MySqlConnection,
    record_id: u32,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<()> {
    sqlx::query(
        "INSERT INTO event_edition_records (record_id, event_id, edition_id)
            VALUES (?, ?, ?)",
    )
    .bind(record_id)
    .bind(event_id)
    .bind(edition_id)
    .execute(conn)
    .await
    .with_api_err()?;

    Ok(())
}

async fn edition_pb(
    MPAuthGuard { login }: MPAuthGuard,
    req_id: RequestId,
    path: Path<(String, u32)>,
    db: Res<Database>,
    body: pb::PbReq,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let (event, edition) = records_lib::must::have_event_edition_with_map(
        &mut mysql_conn,
        &body.map_uid,
        event_handle,
        edition_id,
    )
    .await
    .fit(req_id)?;

    mysql_conn.close().await.with_api_err().fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    pb::pb(login, req_id, db, body, OptEvent::new(&event, &edition)).await
}
