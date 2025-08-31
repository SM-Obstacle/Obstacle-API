mod auth;

use actix_web::{
    HttpResponse, Responder, Scope,
    body::BoxBody,
    web::{self, Json},
};
use entity::{banishments, current_bans, maps, players, records, role};
use futures::TryStreamExt;
use records_lib::{
    Database, ModeVersion, RedisConnection, event, must, opt_event::OptEvent, player, transaction,
};
use reqwest::Client;
use sea_orm::{
    ActiveValue::{Set, Unchanged},
    ColumnTrait as _, ConnectionTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
    StreamTrait, TransactionTrait,
    prelude::Expr,
    sea_query::ExprTrait as _,
};
use serde::{Deserialize, Serialize};
use tracing_actix_web::RequestId;

use crate::{
    FitRequestId as _, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt, Res,
    auth::{ApiAvailable, AuthHeader, MPAuthGuard, privilege},
    utils::{self, ExtractDbConn, json},
};
use dsc_webhook::{WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};

#[cfg(feature = "request_filter")]
use request_filter::{FlagFalseRequest, WebsiteFilter};

use super::{
    pb,
    player_finished::{self as pf, ExpandedInsertRecordParams},
};

pub fn player_scope() -> Scope {
    let scope = web::scope("/player")
        .route("/update", web::post().to(update))
        .route("/finished", web::post().to(finished))
        .route("/get_token", web::post().to(auth::get_token))
        .route("/pb", web::get().to(pb))
        .route("/times", web::post().to(times))
        .route("/info", web::get().to(info))
        .route("/report_error", web::post().to(report_error))
        .route("/ac", web::post().to(ac));

    #[cfg(feature = "request_filter")]
    let scope = scope.service(
        web::scope("")
            .wrap(FlagFalseRequest::<WebsiteFilter>::default())
            .route("/give_token", web::post().to(auth::post_give_token)),
    );
    #[cfg(not(feature = "request_filter"))]
    let scope = scope.route("/give_token", web::post().to(auth::post_give_token));

    scope
}

#[derive(Serialize, Deserialize, Clone, FromQueryResult, Debug)]
pub struct PlayerInfoNetBody {
    pub login: String,
    pub name: String,
    pub zone_path: Option<String>,
}

async fn insert_player<C: ConnectionTrait>(
    conn: &C,
    body: &PlayerInfoNetBody,
) -> RecordsResult<players::Model> {
    let new_player = players::ActiveModel {
        login: Set(body.login.clone()),
        name: Set(body.name.clone()),
        zone_path: Set(body.zone_path.clone()),
        join_date: Set(Some(chrono::Utc::now().naive_utc())),
        admins_note: Set(None),
        role: Set(0),
        ..Default::default()
    };

    players::Entity::insert(new_player)
        .exec_with_returning(conn)
        .await
        .with_api_err()
}

pub async fn get_or_insert<C: ConnectionTrait>(
    conn: &C,
    body: &PlayerInfoNetBody,
) -> RecordsResult<players::Model> {
    if let Some(player) = players::Entity::find()
        .filter(players::Column::Login.eq(&body.login))
        .one(conn)
        .await
        .with_api_err()?
    {
        return Ok(player);
    }

    insert_player(conn, body).await
}

pub async fn update(
    _: ApiAvailable,
    req_id: RequestId,
    db: Res<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<PlayerInfoNetBody>,
) -> RecordsResponse<impl Responder> {
    if body.login != login {
        return Err(RecordsErrorKind::Unauthorized).fit(req_id);
    }

    let mut redis_conn = db.redis_pool.get().await.with_api_err().fit(req_id)?;

    let auth_result = crate::auth::check_auth_for(
        &db.sql_conn,
        &mut redis_conn,
        &login,
        Some(token.as_str()),
        privilege::PLAYER,
    )
    .await;

    match auth_result {
        Ok(id) => update_player(&db.sql_conn, id, body).await.fit(req_id)?,
        // At this point, if Redis has registered a token with the login, it means that
        // the player is not yet added to the Obstacle database but effectively
        // has a ManiaPlanet account
        Err(RecordsErrorKind::Lib(records_lib::error::RecordsError::PlayerNotFound(_))) => {
            let _ = insert_player(&db.sql_conn, &body).await.fit(req_id)?;
        }
        Err(e) => return Err(e).fit(req_id),
    }

    Ok(HttpResponse::Ok().finish())
}

pub async fn update_player<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
    body: PlayerInfoNetBody,
) -> RecordsResult<()> {
    let player = players::Entity::find_by_id(player_id)
        .one(conn)
        .await?
        .unwrap_or_else(|| panic!("Player {player_id} should exist in database"));

    let player_update = players::ActiveModel {
        name: Set(body.name),
        zone_path: Set(body.zone_path),
        join_date: match player.join_date {
            Some(date) => Unchanged(Some(date)),
            None => Set(Some(chrono::Utc::now().naive_utc())),
        },
        ..From::from(player)
    };

    players::Entity::update(player_update).exec(conn).await?;

    Ok(())
}

pub async fn get_ban_during<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
    at: chrono::NaiveDateTime,
) -> RecordsResult<Option<banishments::Model>> {
    banishments::Entity::find()
        .filter(
            banishments::Column::PlayerId
                .eq(player_id)
                .and(banishments::Column::DateBan.lt(at))
                .and(
                    banishments::Column::Duration.is_null().or(Expr::col(
                        banishments::Column::DateBan,
                    )
                    .add(Expr::col(banishments::Column::Duration))
                    .gt(at)),
                ),
        )
        .one(conn)
        .await
        .map_err(From::from)
}

pub async fn check_banned<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
) -> Result<Option<banishments::Model>, RecordsErrorKind> {
    let r = current_bans::Entity::find()
        .filter(current_bans::Column::PlayerId.eq(player_id))
        .one(conn)
        .await
        .with_api_err()?
        .map(From::from);

    Ok(r)
}

async fn finished_impl<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &maps::Model,
) -> RecordsResult<pf::FinishedOutput> {
    let res = pf::finished(conn, redis_conn, params, player_login, map).await?;

    // If the record isn't in an event context, save the record to the events that have the map
    // and allow records saving without an event context.
    let editions = records_lib::event::get_editions_which_contain(conn, map.id)
        .await
        .with_api_err()?
        .try_collect::<Vec<_>>()
        .await
        .with_api_err()?;

    for (event_id, edition_id, original_map_id) in editions {
        super::event::insert_event_record(conn, res.record_id, event_id, edition_id).await?;

        let Some(original_map_id) = original_map_id else {
            continue;
        };

        // Get the previous time of the player on the original map, to check if it would be a PB or not.
        let previous_time =
            player::get_time_on_map(conn, res.player_id, original_map_id, Default::default())
                .await?;
        let is_pb = previous_time.is_none_or(|t| t > params.body.time);

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

    Ok(res)
}

pub async fn finished_at_with_pool(
    db: Database,
    req_id: RequestId,
    mode_version: Option<ModeVersion>,
    login: String,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder> {
    let mut redis_conn = db.redis_pool.get().await.with_api_err().fit(req_id)?;
    let res = finished_at(
        &db.sql_conn,
        &mut redis_conn,
        req_id,
        mode_version,
        login,
        body,
        at,
    )
    .await?;
    Ok(res)
}

pub async fn finished_at<C: TransactionTrait + ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    req_id: RequestId,
    mode_version: Option<ModeVersion>,
    login: String,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder<Body = BoxBody> + use<C>> {
    let map = must::have_map(conn, &body.map_uid)
        .await
        .with_api_err()
        .fit(req_id)?;

    let res: pf::FinishedOutput = transaction::within(conn, async |txn| {
        let params = ExpandedInsertRecordParams {
            body: &body.rest,
            at,
            event: Default::default(),
            mode_version,
        };

        finished_impl(txn, redis_conn, params, &login, &map).await
    })
    .await
    .fit(req_id)?;

    json(res.res)
}

#[inline(always)]
async fn finished(
    _: ApiAvailable,
    mode_version: Option<crate::ModeVersion>,
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    body: pf::PlayerFinishedBody,
) -> RecordsResponse<impl Responder> {
    finished_at_with_pool(
        db.0,
        req_id,
        mode_version.map(|x| x.0),
        login,
        body.0,
        chrono::Utc::now().naive_utc(),
    )
    .await
}

async fn pb(
    _: ApiAvailable,
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    web::Query(body): pb::PbReq,
) -> RecordsResponse<impl Responder> {
    let map = must::have_map(&conn, &body.map_uid)
        .await
        .with_api_err()
        .fit(req_id)?;

    let mut editions = event::get_editions_which_contain(&conn, map.id)
        .await
        .with_api_err()
        .fit(req_id)?;
    let edition = editions.try_next().await.with_api_err().fit(req_id)?;
    let single_edition = editions
        .try_next()
        .await
        .with_api_err()
        .fit(req_id)?
        .is_none();

    drop(editions);

    // FIXME: is this intended? if the player has a PB not in an event that is better than the one in the event,
    // it doesn't return the good one.
    let res = match edition {
        Some((event_id, edition_id, _)) if single_edition => {
            let (event, edition) = must::have_event_edition_from_ids(&conn, event_id, edition_id)
                .await
                .with_api_err()
                .fit(req_id)?;
            pb::pb(
                &conn,
                &login,
                &body.map_uid,
                OptEvent::new(&event, &edition),
            )
            .await
        }
        _ => pb::pb(&conn, &login, &body.map_uid, Default::default()).await,
    }
    .fit(req_id)?;

    utils::json(res)
}

#[derive(Deserialize)]
struct TimesBody {
    maps_uids: Vec<String>,
}

#[derive(Serialize, FromQueryResult)]
struct TimesResponseItem {
    map_uid: String,
    time: i32,
}

async fn times(
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<TimesBody>,
) -> RecordsResponse<impl Responder> {
    let player = records_lib::must::have_player(&conn, &login)
        .await
        .fit(req_id)?;

    let result = maps::Entity::find()
        .reverse_join(records::Entity)
        .filter(
            records::Column::RecordPlayerId
                .eq(player.id)
                .and(maps::Column::GameId.is_in(body.maps_uids)),
        )
        .group_by(maps::Column::Id)
        .select_only()
        .column_as(maps::Column::GameId, "map_uid")
        .column_as(records::Column::Time.min(), "time")
        .into_model::<TimesResponseItem>()
        .all(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(result)
}

#[derive(Deserialize)]
pub struct InfoBody {
    login: String,
}

#[derive(Serialize, FromQueryResult)]
struct InfoResponse {
    id: u32,
    login: String,
    name: String,
    join_date: Option<chrono::NaiveDateTime>,
    zone_path: Option<String>,
    role_name: String,
}

pub async fn info(
    req_id: RequestId,
    ExtractDbConn(conn): ExtractDbConn,
    web::Query(body): web::Query<InfoBody>,
) -> RecordsResponse<impl Responder> {
    let info = players::Entity::find()
        .filter(players::Column::Login.eq(&body.login))
        .inner_join(role::Entity)
        .select_only()
        .columns([
            players::Column::Id,
            players::Column::Login,
            players::Column::Name,
            players::Column::JoinDate,
            players::Column::ZonePath,
        ])
        .column(role::Column::RoleName)
        .into_model::<InfoResponse>()
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    let Some(info) = info else {
        return Err(RecordsErrorKind::from(
            records_lib::error::RecordsError::PlayerNotFound(body.login),
        ))
        .fit(req_id);
    };

    json(info)
}

#[derive(Deserialize)]
struct ReportErrorBody {
    on_route: String,
    request_id: String,
    map_uid: String,
    err_type: i32,
    err_msg: String,
    time: i32,
    respawn_count: i32,
}

async fn report_error(
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    Res(client): Res<Client>,
    Json(body): Json<ReportErrorBody>,
) -> RecordsResponse<impl Responder> {
    let mut fields = vec![
        WebhookBodyEmbedField {
            name: "Map UID".to_owned(),
            value: format!("`{}`", body.map_uid),
            inline: None,
        },
        WebhookBodyEmbedField {
            name: "When called this API route".to_owned(),
            value: format!("`{}`", body.on_route),
            inline: None,
        },
        WebhookBodyEmbedField {
            name: "Request ID".to_owned(),
            value: format!("`{}`", body.request_id),
            inline: None,
        },
    ];

    let (content, color) = if body.on_route == "/player/finished" {
        fields.extend(
            vec![
                WebhookBodyEmbedField {
                    name: "Run time".to_owned(),
                    value: format!("`{}`", body.time),
                    inline: None,
                },
                WebhookBodyEmbedField {
                    name: "Respawn count".to_owned(),
                    value: format!("`{}`", body.respawn_count),
                    inline: None,
                },
            ]
            .into_iter(),
        );

        (
            format!("🚨 Player `{login}` finished a map but got an error."),
            11862016,
        )
    } else {
        (
            format!("⚠️ Player `{login}` got an error while playing."),
            5814783,
        )
    };

    client
        .post(&crate::env().wh_report_url)
        .json(&WebhookBody {
            content,
            embeds: vec![
                WebhookBodyEmbed {
                    title: format!("Error type {}", body.err_type),
                    description: Some(format!("`{}`", body.err_msg)),
                    color,
                    fields: None,
                    url: None,
                },
                WebhookBodyEmbed {
                    title: "Context".to_owned(),
                    description: None,
                    color,
                    fields: Some(fields),
                    url: None,
                },
            ],
        })
        .send()
        .await
        .with_api_err()
        .fit(req_id)?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
struct ACBody {
    run_time: String,
    map_name: String,
    map_uid: String,
    cp_times: String,
    player_field: String,
    server_text: String,
    irl_time_passed: String,
    discrepancy: String,
    discrepancy_ratio: String,
    ac_version: String,
}

async fn ac(
    req_id: RequestId,
    Res(client): Res<Client>,
    Json(body): Json<ACBody>,
) -> RecordsResponse<impl Responder> {
    client
        .post(&crate::env().wh_ac_url)
        .json(&WebhookBody {
            content: format!("Map has been finished in {}", body.run_time),
            embeds: vec![WebhookBodyEmbed {
                title: body.map_name,
                description: Some(body.cp_times),
                color: 5814783,
                url: Some(format!(
                    "https://obstacle.titlepack.io/map/{}",
                    body.map_uid
                )),
                fields: Some(vec![
                    WebhookBodyEmbedField {
                        name: "Player".to_owned(),
                        value: body.player_field,
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Server".to_owned(),
                        value: body.server_text,
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "IRL time elapsed".to_owned(),
                        value: body.irl_time_passed,
                        inline: Some(true),
                    },
                    WebhookBodyEmbedField {
                        name: "Discrepancy".to_owned(),
                        value: body.discrepancy,
                        inline: Some(true),
                    },
                    WebhookBodyEmbedField {
                        name: "Discrepancy ratio".to_owned(),
                        value: body.discrepancy_ratio,
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Anticheat version".to_owned(),
                        value: body.ac_version,
                        inline: None,
                    },
                ]),
            }],
        })
        .send()
        .await
        .with_api_err()
        .fit(req_id)?;

    Ok(HttpResponse::Ok().finish())
}
