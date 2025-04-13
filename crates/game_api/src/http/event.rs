use std::collections::HashMap;

use actix_web::{
    Responder, Scope,
    web::{self, Path, Query},
};
use futures::TryStreamExt;
use itertools::Itertools;
use records_lib::{
    Database, DatabaseConnection, MySqlConnection, NullableInteger, NullableReal, NullableText,
    RedisConnection, acquire,
    context::{
        Context, Ctx as _, HasEditionId, HasEventId, HasEventIds, HasMap, HasPlayerLogin,
        ReadWrite, Transactional,
    },
    error::RecordsError,
    event::{self, EventMap},
    models, player, transaction,
};
use serde::Serialize;
use sqlx::FromRow;
use tracing_actix_web::RequestId;

use crate::{
    FitRequestId, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt, Res,
    auth::MPAuthGuard,
    utils::{self, json},
};

use super::{
    overview, pb,
    player::PlayerInfoNetBody,
    player_finished::{self as pf, HasFinishedBody},
};

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
    mx_id: Option<i64>,
    original_map_id: Option<u32>,
}

async fn get_maps_by_edition_id(
    db: &Database,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<MapWithCategory>> {
    let r = sqlx::query_as(
        "SELECT m.*, category_id, mx_id, original_map_id
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

#[derive(sqlx::FromRow)]
struct RawOriginalMap {
    original_mx_id: Option<i64>,
    #[sqlx(flatten)]
    map: models::Map,
}

#[derive(Serialize, Default)]
struct OriginalMap {
    mx_id: NullableInteger<0>,
    name: NullableText,
    map_uid: NullableText,
}

#[derive(Serialize)]
struct Map {
    mx_id: NullableInteger<0>,
    main_author: PlayerInfoNetBody,
    name: String,
    map_uid: String,
    bronze_time: NullableInteger,
    silver_time: NullableInteger,
    gold_time: NullableInteger,
    champion_time: NullableInteger,
    personal_best: NullableInteger,
    next_opponent: NextOpponent,
    original_map: OriginalMap,
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

#[derive(Serialize)]
struct EventEditionInGameParams {
    titles_align: NullableText,
    lb_link_align: NullableText,
    authors_align: NullableText,
    put_subtitle_on_newline: bool,
    titles_pos_x: NullableReal,
    titles_pos_y: NullableReal,
    lb_link_pos_x: NullableReal,
    lb_link_pos_y: NullableReal,
    authors_pos_x: NullableReal,
    authors_pos_y: NullableReal,
}

/// Converts the given "raw" alignment retrieved from the DB, into the alignment that will be received
/// by the Titlepack.
///
/// If X or Y positions are precised, then they're used instead of the given alignment.
fn db_align_to_mp_align(
    alignment: Option<models::InGameAlignment>,
    pos_x: Option<f64>,
    pos_y: Option<f64>,
) -> NullableText {
    match alignment {
        None if pos_x.is_none() && pos_y.is_none() => {
            Some(records_lib::env().ingame_default_titles_align)
        }
        Some(_) if pos_x.is_some() || pos_y.is_some() => None,
        other => other,
    }
    .map(|p| p.to_char().to_string())
    .into()
}

impl From<models::InGameEventEditionParams> for EventEditionInGameParams {
    fn from(value: models::InGameEventEditionParams) -> Self {
        Self {
            titles_align: db_align_to_mp_align(
                value.titles_align,
                value.titles_pos_x,
                value.titles_pos_y,
            ),
            lb_link_align: db_align_to_mp_align(
                value.lb_link_align,
                value.lb_link_pos_x,
                value.lb_link_pos_y,
            ),
            authors_align: db_align_to_mp_align(
                value.authors_align,
                value.authors_pos_x,
                value.authors_pos_y,
            ),
            put_subtitle_on_newline: value
                .put_subtitle_on_newline
                .unwrap_or_else(|| records_lib::env().ingame_default_subtitle_on_newline),
            titles_pos_x: value.titles_pos_x.into(),
            titles_pos_y: value.titles_pos_y.into(),
            lb_link_pos_x: value.lb_link_pos_x.into(),
            lb_link_pos_y: value.lb_link_pos_y.into(),
            authors_pos_x: value.authors_pos_x.into(),
            authors_pos_y: value.authors_pos_y.into(),
        }
    }
}

impl Default for EventEditionInGameParams {
    fn default() -> Self {
        Self {
            titles_align: NullableText(Some(
                records_lib::env()
                    .ingame_default_titles_align
                    .to_char()
                    .to_string(),
            )),
            lb_link_align: NullableText(Some(
                records_lib::env()
                    .ingame_default_lb_link_align
                    .to_char()
                    .to_string(),
            )),
            authors_align: NullableText(Some(
                records_lib::env()
                    .ingame_default_authors_align
                    .to_char()
                    .to_string(),
            )),
            put_subtitle_on_newline: records_lib::env().ingame_default_subtitle_on_newline,
            titles_pos_x: NullableReal(None),
            titles_pos_y: NullableReal(None),
            lb_link_pos_x: NullableReal(None),
            lb_link_pos_y: NullableReal(None),
            authors_pos_x: NullableReal(None),
            authors_pos_y: NullableReal(None),
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
    /// The UTC timestamp of the edition start date.
    start_date: i32,
    /// The UTC timestamp of the edition end date.
    ///
    /// It is `None` if the edition never ends.
    end_date: NullableInteger,
    ingame_params: EventEditionInGameParams,
    /// The URL to the banner image of the edition, used in the Titlepack menu.
    banner_img_url: String,
    /// The URL to the small banner image of the edition, used in game in the Campaign mode.
    banner2_img_url: String,
    /// The MX ID of the related mappack.
    mx_id: NullableInteger,
    /// Whether the edition has expired or not.
    expired: bool,
    original_map_uids: Vec<String>,
    /// The content of the edition (the categories).
    ///
    /// If the event has no category, the maps are grouped in a single category with all
    /// its fields empty.
    categories: Vec<Category>,
}

#[derive(serde::Deserialize)]
struct EventListQuery {
    #[serde(default)]
    include_expired: bool,
}

async fn event_list(
    req_id: RequestId,
    db: Res<Database>,
    Query(EventListQuery { include_expired }): Query<EventListQuery>,
) -> RecordsResponse<impl Responder> {
    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let out = event::event_list(&mut mysql_conn, !include_expired)
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

    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;
    let ctx = Context::default().with_event_handle(&event_handle);

    let id = records_lib::must::have_event_handle(&mut mysql_conn, &ctx)
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
    .fetch_all(&mut *mysql_conn)
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
    login: NullableText,
    name: NullableText,
    time: NullableInteger,
}

struct AuthorWithPlayerTime {
    /// The author of the map
    main_author: PlayerInfoNetBody,
    /// The time of the player (not the same player as the author)
    personal_best: NullableInteger,
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

    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;
    let ctx = Context::default()
        .with_event_handle(&event_handle)
        .with_edition_id(edition_id);

    let (event, edition) = records_lib::must::have_event_edition(&mut mysql_conn, &ctx)
        .await
        .fit(req_id)?;

    let ctx = ctx.with_event_edition(&event, &edition);

    // The edition is not yet released
    if chrono::Utc::now() < edition.start_date.and_utc() {
        return Err(RecordsError::EventEditionNotFound(event_handle, edition_id))
            .with_api_err()
            .fit(req_id);
    }

    let maps = get_maps_by_edition_id(&db, event.id, edition_id)
        .await
        .fit(req_id)?
        .into_iter()
        .chunk_by(|m| m.category_id);
    let maps = maps.into_iter();

    let mut cat = event::get_categories_by_edition_id(
        &mut mysql_conn,
        ctx.get_event_id(),
        ctx.get_edition_id(),
    )
    .await
    .fit(req_id)?;

    let original_maps: Vec<RawOriginalMap> = sqlx::query_as(
        r#"select m.*, eem.original_mx_id from event_edition_maps eem
        inner join maps m on m.id = eem.original_map_id
        where eem.event_id = ? and eem.edition_id = ? and eem.transitive_save = TRUE"#,
    )
    .bind(edition.event_id)
    .bind(edition.id)
    .fetch_all(&db.mysql_pool)
    .await
    .with_api_err()
    .fit(req_id)?;

    let original_maps = original_maps
        .into_iter()
        .map(|map| (map.map.id, map))
        .collect::<HashMap<_, _>>();

    let mut categories = Vec::with_capacity(cat.len());

    for (cat_id, cat_maps) in maps {
        let m = cat_id
            .and_then(|c_id| cat.iter().find_position(|c| c.id == c_id))
            .map(|(i, _)| i)
            .map(|i| cat.swap_remove(i))
            .unwrap_or_default();

        let mut maps = Vec::with_capacity(cat_maps.size_hint().0);

        for MapWithCategory {
            map,
            mx_id,
            original_map_id,
            ..
        } in cat_maps
        {
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
                .bind(event.id)
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
                .bind(event.id)
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
                    personal_best: None.into(),
                    next_opponent: None,
                }
            };

            let medal_times =
                event::get_medal_times_of(&mut mysql_conn, ctx.by_ref().with_map_id(map.id))
                    .await
                    .with_api_err()
                    .fit(req_id)?;

            let original_map = original_map_id
                .and_then(|id| original_maps.get(&id))
                .map(|map| OriginalMap {
                    mx_id: NullableInteger(map.original_mx_id.map(|x| x as i32)),
                    name: NullableText(Some(map.map.name.clone())),
                    map_uid: NullableText(Some(map.map.game_id.clone())),
                })
                .unwrap_or_default();

            maps.push(Map {
                mx_id: mx_id.map(|id| id as _).into(),
                main_author,
                name: map.name,
                map_uid: map.game_id,
                bronze_time: medal_times.map(|m| m.bronze_time).into(),
                silver_time: medal_times.map(|m| m.silver_time).into(),
                gold_time: medal_times.map(|m| m.gold_time).into(),
                champion_time: medal_times.map(|m| m.champion_time).into(),
                original_map,
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

    let ingame_params = edition
        .get_ingame_params(&mut mysql_conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .map(EventEditionInGameParams::from)
        .unwrap_or_default();

    let res = EventHandleEditionResponse {
        expired: edition.has_expired(),
        end_date: edition
            .expire_date()
            .map(|d| d.and_utc().timestamp() as _)
            .into(),
        id: edition.id,
        authors: event::get_admins_of(&mut mysql_conn, ctx.get_event_id(), ctx.get_edition_id())
            .map_ok(|p| p.name)
            .try_collect()
            .await
            .with_api_err()
            .fit(req_id)?,
        name: edition.name,
        subtitle: edition.subtitle.unwrap_or_default(),
        start_date: edition.start_date.and_utc().timestamp() as _,
        ingame_params,
        banner_img_url: edition.banner_img_url.unwrap_or_default(),
        banner2_img_url: edition.banner2_img_url.unwrap_or_default(),
        mx_id: edition.mx_id.map(|id| id as _).into(),
        original_map_uids: original_maps
            .into_values()
            .map(|map| map.map.game_id)
            .collect(),
        categories,
    };

    json(res)
}

async fn edition_overview(
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    query: overview::OverviewReq,
) -> RecordsResponse<impl Responder> {
    let conn = acquire!(db.with_api_err().fit(req_id)?);
    let (event, edition) = path.into_inner();
    let ctx = Context::default()
        .with_event_handle(&event)
        .with_edition_id(edition)
        .with_map_uid(&query.map_uid)
        .with_player_login(&query.login);

    let (event, edition, EventMap { map, .. }) =
        records_lib::must::have_event_edition_with_map(conn.mysql_conn, &ctx)
            .await
            .with_api_err()
            .fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res = overview::overview(
        conn,
        ctx.with_event_edition(&event, &edition).with_map(&map),
    )
    .await
    .fit(req_id)?;

    utils::json(res)
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

async fn edition_finished_impl<C>(
    mysql_conn: MySqlConnection<'_>,
    redis_conn: &mut RedisConnection,
    ctx: C,
    original_map_id: Option<u32>,
    at: chrono::NaiveDateTime,
    body: HasFinishedBody,
) -> RecordsResult<pf::FinishedOutput>
where
    C: HasPlayerLogin + HasMap + HasEventIds + Transactional<Mode = ReadWrite>,
{
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn,
    };

    // Then we insert the record for the global records
    let res = pf::finished(&mut conn, &ctx, body.rest.clone(), at).await?;

    if let Some(original_map_id) = original_map_id {
        let ctx = ctx
            .by_ref()
            .with_player_id(res.player_id)
            .with_map_id(original_map_id)
            .with_no_event();

        // Get the previous time of the player on the original map to check if it's a PB
        let time_on_previous = player::get_time_on_map(conn.mysql_conn, &ctx)
            .await
            .with_api_err()?;
        let is_pb =
            time_on_previous.is_none() || time_on_previous.is_some_and(|t| t > body.rest.time);

        // Here, we don't provide the event instances, because we don't want to save in event mode.
        pf::insert_record(&mut conn, &ctx, &body.rest, Some(res.record_id), at, is_pb).await?;
    }

    // Then we insert it for the event edition records.
    insert_event_record(
        conn.mysql_conn,
        res.record_id,
        ctx.get_event_id(),
        ctx.get_edition_id(),
    )
    .await?;

    Ok(res)
}

pub async fn edition_finished_at(
    login: String,
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder> {
    let conn = acquire!(db.with_api_err().fit(req_id)?);

    let (event_handle, edition_id) = path.into_inner();

    let ctx = Context::default()
        .with_event_handle(&event_handle)
        .with_edition_id(edition_id)
        .with_player_login(&login);

    // We first check that the event and its edition exist
    // and that the map is registered on it.
    let (
        event,
        edition,
        EventMap {
            map,
            original_map_id,
        },
    ) = records_lib::must::have_event_edition_with_map(
        conn.mysql_conn,
        ctx.by_ref().with_map_uid(&body.map_uid),
    )
    .await
    .fit(req_id)?;

    let ctx = ctx.with_event_edition(&event, &edition).with_map(&map);

    if edition.has_expired()
        && !(edition.start_date <= at && edition.expire_date().filter(|date| at > *date).is_none())
    {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res: pf::FinishedOutput = transaction::within(
        conn.mysql_conn,
        ctx.with_event_edition(&event, &edition).with_map(&map),
        ReadWrite,
        async |mysql_conn, ctx| {
            edition_finished_impl(mysql_conn, conn.redis_conn, ctx, original_map_id, at, body).await
        },
    )
    .await
    .fit(req_id)?;

    json(res.res)
}

pub async fn insert_event_record(
    conn: &mut sqlx::MySqlConnection,
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
    let ctx = Context::default()
        .with_event_handle(&event_handle)
        .with_edition_id(edition_id)
        .with_map_uid(&body.map_uid);

    let mut mysql_conn = db.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let (event, edition, EventMap { map, .. }) =
        records_lib::must::have_event_edition_with_map(&mut mysql_conn, &ctx)
            .await
            .fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res = pb::pb(
        Context::default()
            .with_map(&map)
            .with_player_login(&login)
            .with_mysql_pool(db.0.mysql_pool),
    )
    .await
    .fit(req_id)?;

    utils::json(res)
}
