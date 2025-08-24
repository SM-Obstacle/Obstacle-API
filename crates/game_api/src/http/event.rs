use std::collections::HashMap;

use actix_web::{
    Responder, Scope,
    web::{self, Path},
};
use entity::{
    event_edition, event_edition_maps, event_edition_records, global_event_records, global_records,
    in_game_event_edition_params, maps, players, records, types::InGameAlignment,
};
use futures::TryStreamExt;
use itertools::Itertools;
use records_lib::{
    Database, NullableInteger, NullableReal, NullableText, RedisConnection,
    error::RecordsError,
    event::{self, EventMap},
    opt_event::OptEvent,
    player, transaction,
};
use sea_orm::{
    ActiveValue::Set,
    ColumnTrait as _, ConnectionTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait as _, RelationTrait as _, StatementBuilder, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, Func, Query},
};
use serde::Serialize;
use tracing_actix_web::RequestId;

use crate::{
    FitRequestId, ModeVersion, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt,
    Res,
    auth::MPAuthGuard,
    utils::{self, ExtractDbConn, json},
};

use super::{
    overview, pb,
    player::PlayerInfoNetBody,
    player_finished::{self as pf, ExpandedInsertRecordParams},
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

#[derive(FromQueryResult)]
struct MapWithCategory {
    #[sea_orm(nested)]
    map: maps::Model,
    category_id: Option<u32>,
    mx_id: Option<i64>,
    original_map_id: Option<u32>,
}

async fn get_maps_by_edition_id<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<MapWithCategory>> {
    let r = maps::Entity::find()
        .join_rev(
            sea_orm::JoinType::InnerJoin,
            event_edition_maps::Relation::Maps2.def(),
        )
        .order_by_asc(event_edition_maps::Column::CategoryId)
        .order_by_asc(event_edition_maps::Column::Order)
        .filter(
            event_edition_maps::Column::EventId
                .eq(event_id)
                .and(event_edition_maps::Column::EditionId.eq(edition_id)),
        )
        .columns([
            event_edition_maps::Column::CategoryId,
            event_edition_maps::Column::MxId,
            event_edition_maps::Column::OriginalMapId,
        ])
        .into_model()
        .all(conn)
        .await
        .with_api_err()?;

    Ok(r)
}

#[derive(Serialize, FromQueryResult)]
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

#[derive(FromQueryResult)]
struct RawOriginalMap {
    original_mx_id: Option<i64>,
    #[sea_orm(nested)]
    map: maps::Model,
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
    handle: NullableText,
    name: NullableText,
    banner_img_url: NullableText,
    hex_color: NullableText,
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
    alignment: Option<InGameAlignment>,
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

impl From<in_game_event_edition_params::Model> for EventEditionInGameParams {
    fn from(value: in_game_event_edition_params::Model) -> Self {
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
                .map(|b| b != 0)
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
    ExtractDbConn(conn): ExtractDbConn,
    web::Query(EventListQuery { include_expired }): web::Query<EventListQuery>,
) -> RecordsResponse<impl Responder> {
    let out = event::event_list(&conn, !include_expired)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(out)
}

async fn event_editions(
    ExtractDbConn(conn): ExtractDbConn,
    req_id: RequestId,
    event_handle: Path<String>,
) -> RecordsResponse<impl Responder> {
    let event_handle = event_handle.into_inner();

    let id = records_lib::must::have_event_handle(&conn, &event_handle)
        .await
        .fit(req_id)?
        .id;

    let res = event_edition::Entity::find()
        .filter(
            event_edition::Column::EventId
                .eq(id)
                .and(Expr::col(event_edition::Column::StartDate).lt(Func::cust("SYSDATE")))
                .and(
                    Expr::tuple([
                        Expr::col(event_edition::Column::EventId).into(),
                        Expr::col(event_edition::Column::Id).into(),
                    ])
                    .in_subquery(
                        Query::select()
                            .from(event_edition_maps::Entity)
                            .columns([
                                event_edition_maps::Column::EventId,
                                event_edition_maps::Column::EditionId,
                            ])
                            .take(),
                    ),
                ),
        )
        .order_by_desc(event_edition::Column::Id)
        .into_model::<RawEventHandleResponse>()
        .all(&conn)
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

#[derive(FromQueryResult, Serialize, Default)]
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
    ExtractDbConn(conn): ExtractDbConn,
    req_id: RequestId,
    path: Path<(String, u32)>,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    let (event, edition) = records_lib::must::have_event_edition(&conn, &event_handle, edition_id)
        .await
        .fit(req_id)?;

    // The edition is not yet released
    if chrono::Utc::now() < edition.start_date.and_utc() {
        return Err(RecordsError::EventEditionNotFound(event_handle, edition_id))
            .with_api_err()
            .fit(req_id);
    }

    let maps = get_maps_by_edition_id(&conn, event.id, edition_id)
        .await
        .fit(req_id)?
        .into_iter()
        .chunk_by(|m| m.category_id);
    let maps = maps.into_iter();

    let mut input_categories = event::get_categories_by_edition_id(&conn, event.id, edition.id)
        .await
        .fit(req_id)?;

    let original_maps = event_edition_maps::Entity::find()
        .join(
            sea_orm::JoinType::InnerJoin,
            event_edition_maps::Relation::Maps1.def(),
        )
        .filter(
            event_edition_maps::Column::EventId
                .eq(edition.event_id)
                .and(event_edition_maps::Column::EditionId.eq(edition.id))
                .and(
                    Expr::col(event_edition_maps::Column::TransitiveSave)
                        .eq(Expr::custom_keyword("TRUE")),
                ),
        )
        .select_only()
        .column_as(Expr::col((maps::Entity, Asterisk)), "map")
        .column(event_edition_maps::Column::OriginalMxId)
        .into_model::<RawOriginalMap>()
        .all(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    let original_maps = original_maps
        .into_iter()
        .map(|map| (map.map.id, map))
        .collect::<HashMap<_, _>>();

    let mut output_categories = Vec::with_capacity(input_categories.len());

    for (cat_id, cat_maps) in maps {
        let category = cat_id
            .and_then(|c_id| input_categories.iter().find_position(|c| c.id == c_id))
            .map(|(i, _)| i)
            .map(|i| input_categories.swap_remove(i));

        let mut maps = Vec::with_capacity(cat_maps.size_hint().0);

        for MapWithCategory {
            map,
            mx_id,
            original_map_id,
            ..
        } in cat_maps
        {
            let main_author = players::Entity::find_by_id(map.player_id)
                .select_only()
                .columns([
                    players::Column::Login,
                    players::Column::Name,
                    players::Column::ZonePath,
                ])
                .into_model()
                .one(&conn)
                .await
                .with_api_err()
                .fit(req_id)?
                .expect("Main map author must exist");

            let AuthorWithPlayerTime {
                main_author,
                personal_best,
                next_opponent,
            } = if let Some(MPAuthGuard { login }) = &auth {
                let personal_best = records::Entity::find()
                    .inner_join(players::Entity)
                    .filter(
                        players::Column::Login
                            .eq(login)
                            .and(records::Column::MapId.eq(map.id)),
                    )
                    .apply_if((edition.is_transparent == 0).then_some(()), |q, _| {
                        q.reverse_join(event_edition_records::Entity).filter(
                            event_edition_records::Column::EventId
                                .eq(event.id)
                                .and(event_edition_records::Column::EditionId.eq(edition_id)),
                        )
                    })
                    .select_only()
                    .column_as(records::Column::Time.min(), "pb_time")
                    .into_tuple::<Option<i32>>()
                    .one(&conn)
                    .await
                    .with_api_err()
                    .fit(req_id)?
                    .flatten()
                    .into();

                let mut next_opponent_query = Query::select();
                let next_opponent_query = next_opponent_query.join_as(
                    sea_orm::JoinType::InnerJoin,
                    players::Entity,
                    "player_from",
                    Expr::col(("player_from", players::Column::Id))
                        .eq(Expr::col(("gr", records::Column::RecordPlayerId))),
                );

                if edition.is_transparent == 0 {
                    next_opponent_query
                        .from_as(global_event_records::Entity, "gr")
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            global_event_records::Entity,
                            "gr2",
                            Expr::col(("gr2", global_event_records::Column::MapId))
                                .eq(Expr::col(("gr", global_event_records::Column::MapId)))
                                .and(
                                    Expr::col(("gr2", global_event_records::Column::Time))
                                        .lt(Expr::col(("gr", global_event_records::Column::Time))),
                                )
                                .and(
                                    Expr::col(("gr2", global_event_records::Column::EventId)).eq(
                                        Expr::col(("gr", global_event_records::Column::EventId)),
                                    ),
                                )
                                .and(
                                    Expr::col(("gr2", global_event_records::Column::EditionId)).eq(
                                        Expr::col(("gr", global_event_records::Column::EditionId)),
                                    ),
                                ),
                        )
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            players::Entity,
                            "p",
                            Expr::col(("p", players::Column::Id)).eq(Expr::col((
                                "gr2",
                                global_event_records::Column::RecordPlayerId,
                            ))),
                        )
                        .and_where(
                            Expr::col(("gr", global_event_records::Column::MapId)).eq(map.id),
                        )
                        .and_where(
                            Expr::col(("gr", global_event_records::Column::EventId))
                                .eq(event.id)
                                .and(
                                    Expr::col(("gr", global_event_records::Column::EditionId))
                                        .eq(edition.id),
                                ),
                        )
                        .expr(Expr::col(("gr2", global_event_records::Column::Time)));
                } else {
                    next_opponent_query
                        .from_as(global_records::Entity, "gr")
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            global_records::Entity,
                            "gr2",
                            Expr::col(("gr2", global_records::Column::MapId))
                                .eq(Expr::col(("gr", global_records::Column::MapId)))
                                .and(
                                    Expr::col(("gr2", global_records::Column::Time))
                                        .lt(Expr::col(("gr", global_records::Column::Time))),
                                ),
                        )
                        // This join allows us to filter later on the event ID and edition ID.
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            event_edition_maps::Entity,
                            "eem",
                            Expr::col(("gr", global_records::Column::MapId))
                                .eq(Expr::col(("eem", maps::Column::Id))),
                        )
                        // TODO: try to remove this join that is similar to the one in the above branch
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            players::Entity,
                            "p",
                            Expr::col(("p", players::Column::Id))
                                .eq(Expr::col(("gr2", global_records::Column::RecordPlayerId))),
                        )
                        // TODO: same
                        .and_where(Expr::col(("gr", global_records::Column::MapId)).eq(map.id))
                        .and_where(
                            Expr::col(("eem", event_edition_maps::Column::EventId))
                                .eq(event.id)
                                .and(
                                    Expr::col(("eem", event_edition_maps::Column::EditionId))
                                        .eq(edition.id),
                                ),
                        )
                        // TODO: same
                        .expr(Expr::col(("gr2", global_records::Column::Time)));
                }

                next_opponent_query
                    .and_where(Expr::col(("player_from", players::Column::Login)).eq(login))
                    .exprs([
                        Expr::col(("p", players::Column::Login)),
                        Expr::col(("p", players::Column::Name)),
                    ]);

                let next_opponent_query =
                    StatementBuilder::build(&*next_opponent_query, &conn.get_database_backend());

                let next_opponent = conn
                    .query_one(next_opponent_query)
                    .await
                    .with_api_err()
                    .fit(req_id)?
                    .map(|result| NextOpponent::from_query_result(&result, ""))
                    .transpose()
                    .with_api_err()
                    .fit(req_id)?;

                AuthorWithPlayerTime {
                    main_author,
                    personal_best,
                    next_opponent,
                }
            } else {
                AuthorWithPlayerTime {
                    main_author,
                    personal_best: None.into(),
                    next_opponent: None,
                }
            };

            let medal_times = event::get_medal_times_of(&conn, event.id, edition.id, map.id)
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

        // meow
        let cat = match category {
            Some(cat) => Category {
                handle: Some(cat.handle).into(),
                name: Some(cat.name).into(),
                banner_img_url: cat.banner_img_url.into(),
                hex_color: cat.hex_color.into(),
                maps,
            },
            None => Category {
                maps,
                ..Default::default()
            },
        };

        output_categories.push(cat);
    }

    // Fill the remaining with empty categories
    for cat in input_categories {
        output_categories.push(Category {
            handle: Some(cat.handle).into(),
            name: Some(cat.name).into(),
            banner_img_url: cat.banner_img_url.into(),
            hex_color: cat.hex_color.into(),
            maps: Vec::new(),
        });
    }

    let ingame_params = edition
        .get_ingame_params(&conn)
        .await
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
        authors: event::get_admins_of(&conn, event.id, edition.id)
            .await
            .with_api_err()
            .fit(req_id)?
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
        categories: output_categories,
    };

    json(res)
}

pub trait HasExpireTime {
    /// Returns the UTC expire date.
    fn expire_date(&self) -> Option<chrono::NaiveDateTime>;

    /// Returns the number of seconds until the edition expires from now.
    ///
    /// If the edition doesn't expire (it hasn't a TTL), it returns `None`.
    fn expires_in(&self) -> Option<i64> {
        self.expire_date()
            .map(|d| (d - chrono::Utc::now().naive_utc()).num_seconds())
    }

    /// Returns whether the edition has expired or not.
    fn has_expired(&self) -> bool {
        self.expires_in().filter(|n| *n < 0).is_some()
    }
}

trait EventEditionTraitExt {
    /// Returns the additional in-game parameters of the provided event edition.
    async fn get_ingame_params<C: ConnectionTrait>(
        &self,
        conn: &C,
    ) -> RecordsResult<Option<in_game_event_edition_params::Model>>;
}

impl HasExpireTime for event_edition::Model {
    fn expire_date(&self) -> Option<chrono::NaiveDateTime> {
        self.ttl.and_then(|ttl| {
            self.start_date
                .checked_add_signed(chrono::Duration::seconds(ttl as _))
        })
    }
}

impl EventEditionTraitExt for event_edition::Model {
    async fn get_ingame_params<C: ConnectionTrait>(
        &self,
        conn: &C,
    ) -> RecordsResult<Option<in_game_event_edition_params::Model>> {
        let Some(id) = self.ingame_params_id else {
            return Ok(None);
        };

        let params = in_game_event_edition_params::Entity::find_by_id(id)
            .one(conn)
            .await
            .with_api_err()?;

        Ok(params)
    }
}

async fn edition_overview(
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    query: overview::OverviewReq,
) -> RecordsResponse<impl Responder> {
    let mut redis_conn = db.0.redis_pool.get().await.with_api_err().fit(req_id)?;

    let (event, edition) = path.into_inner();

    let (event, edition, EventMap { map, .. }) = records_lib::must::have_event_edition_with_map(
        &db.sql_conn,
        &query.map_uid,
        &event,
        edition,
    )
    .await
    .with_api_err()
    .fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res = overview::overview(
        &db.sql_conn,
        &mut redis_conn,
        &query.login,
        &map,
        OptEvent::new(&event, &edition),
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
    mode_version: ModeVersion,
) -> RecordsResponse<impl Responder> {
    edition_finished_at(
        login,
        req_id,
        db,
        path,
        body.0,
        chrono::Utc::now().naive_utc(),
        Some(mode_version.0),
    )
    .await
}

async fn edition_finished_impl<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &maps::Model,
    event_id: u32,
    edition_id: u32,
    original_map_id: Option<u32>,
) -> RecordsResult<pf::FinishedOutput> {
    // Then we insert the record for the global records
    let res = pf::finished(conn, redis_conn, params, player_login, map).await?;

    if let Some(original_map_id) = original_map_id {
        // Get the previous time of the player on the original map to check if it's a PB
        let time_on_previous =
            player::get_time_on_map(conn, res.player_id, original_map_id, Default::default())
                .await
                .with_api_err()?;
        let is_pb =
            time_on_previous.is_none() || time_on_previous.is_some_and(|t| t > params.body.time);

        // Here, we don't provide the event instances, because we don't want to save in event mode.
        pf::insert_record(
            conn,
            redis_conn,
            ExpandedInsertRecordParams {
                event: Default::default(),
                ..params
            },
            original_map_id,
            res.player_id,
            Some(res.record_id),
            is_pb,
        )
        .await?;
    }

    // Then we insert it for the event edition records.
    insert_event_record(conn, res.record_id, event_id, edition_id).await?;

    Ok(res)
}

pub async fn edition_finished_at(
    login: String,
    req_id: RequestId,
    db: Res<Database>,
    path: Path<(String, u32)>,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
    mode_version: Option<records_lib::ModeVersion>,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

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
        &db.sql_conn,
        &body.map_uid,
        &event_handle,
        edition_id,
    )
    .await
    .fit(req_id)?;

    let mut redis_conn = db.0.redis_pool.get().await.with_api_err().fit(req_id)?;

    // The edition is transparent, so we save the record for the map directly.
    if edition.is_transparent != 0 {
        let res = super::player::finished_at(
            &db.sql_conn,
            &mut redis_conn,
            req_id,
            mode_version,
            login,
            body,
            at,
        )
        .await?;
        return Ok(utils::Either::Left(res));
    }

    if edition.has_expired()
        && !(edition.start_date <= at && edition.expire_date().filter(|date| at > *date).is_none())
    {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res: pf::FinishedOutput = transaction::within(&db.sql_conn, async |txn| {
        let params = ExpandedInsertRecordParams {
            body: &body.rest,
            at,
            event: OptEvent::new(&event, &edition),
            mode_version,
        };

        edition_finished_impl(
            txn,
            &mut redis_conn,
            params,
            &login,
            &map,
            event.id,
            edition.id,
            original_map_id,
        )
        .await
    })
    .await
    .fit(req_id)?;

    json(res.res).map(utils::Either::Right)
}

pub async fn insert_event_record<C: ConnectionTrait>(
    conn: &C,
    record_id: u32,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<()> {
    event_edition_records::Entity::insert(event_edition_records::ActiveModel {
        record_id: Set(record_id),
        event_id: Set(event_id),
        edition_id: Set(edition_id),
    })
    .exec(conn)
    .await
    .with_api_err()?;

    Ok(())
}

async fn edition_pb(
    MPAuthGuard { login }: MPAuthGuard,
    req_id: RequestId,
    path: Path<(String, u32)>,
    ExtractDbConn(conn): ExtractDbConn,
    body: pb::PbReq,
) -> RecordsResponse<impl Responder> {
    let (event_handle, edition_id) = path.into_inner();

    let (event, edition, EventMap { map, .. }) = records_lib::must::have_event_edition_with_map(
        &conn,
        &body.map_uid,
        &event_handle,
        edition_id,
    )
    .await
    .fit(req_id)?;

    if edition.has_expired() {
        return Err(RecordsErrorKind::EventHasExpired(event.handle, edition.id)).fit(req_id);
    }

    let res = pb::pb(&conn, &login, &map.game_id, OptEvent::new(&event, &edition))
        .await
        .fit(req_id)?;

    utils::json(res)
}
