#[cfg(auth)]
use actix_session::Session;
#[cfg(auth)]
use actix_web::web::Data;
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
#[cfg(auth)]
use reqwest::StatusCode;
use sea_orm::{
    ActiveValue::Set,
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait, FromQueryResult, QueryFilter,
    QuerySelect, StatementBuilder, StreamTrait, TransactionTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Query},
};
use serde::{Deserialize, Serialize};
#[cfg(auth)]
use tokio::time::timeout;
#[cfg(auth)]
use tracing::Level;
use tracing_actix_web::RequestId;

#[cfg(auth)]
use crate::{
    AccessTokenErr,
    auth::{AuthState, Message, TIMEOUT, WEB_TOKEN_SESS_KEY, WebToken},
};

use crate::{
    FitRequestId as _, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt, Res,
    auth::{self, ApiAvailable, AuthHeader, MPAuthGuard, privilege},
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
        .route("/get_token", web::post().to(get_token))
        .route("/pb", web::get().to(pb))
        .route("/times", web::post().to(times))
        .route("/info", web::get().to(info))
        .route("/report_error", web::post().to(report_error))
        .route("/ac", web::post().to(ac));

    #[cfg(feature = "request_filter")]
    let scope = scope.service(
        web::scope("")
            .wrap(FlagFalseRequest::<WebsiteFilter>::default())
            .route("/give_token", web::post().to(post_give_token)),
    );
    #[cfg(not(feature = "request_filter"))]
    let scope = scope.route("/give_token", web::post().to(post_give_token));

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
    let auth_check = auth::check_auth_for(&db, &login, &token, privilege::PLAYER).await;
    let conn = DbConn::from(db.0.mysql_pool);

    match auth_check {
        Ok(id) => update_player(&conn, id, body).await.fit(req_id)?,
        // At this point, if Redis has registered a token with the login, it means that
        // the player is not yet added to the Obstacle database but effectively
        // has a ManiaPlanet account
        Err(RecordsErrorKind::Lib(records_lib::error::RecordsError::PlayerNotFound(_))) => {
            let _ = insert_player(&conn, &body).await.fit(req_id)?;
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
    let mut update = Query::update();
    let update = update
        .table(players::Entity)
        .and_where(players::Column::Id.eq(player_id))
        .value(players::Column::Name, body.name)
        .value(players::Column::ZonePath, body.zone_path);
    let stmt = StatementBuilder::build(&*update, &conn.get_database_backend());
    conn.execute(stmt).await.with_api_err()?;

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

#[cfg(auth)]
#[derive(Serialize)]
struct MPAccessTokenBody<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
}

#[cfg(auth)]
#[derive(Deserialize)]
#[serde(untagged)]
enum MPAccessTokenResponse {
    AccessToken { access_token: String },
    Error(AccessTokenErr),
}

#[cfg(auth)]
#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

#[cfg(auth)]
async fn test_access_token(
    client: &Client,
    login: &str,
    code: &str,
    redirect_uri: &str,
) -> RecordsResult<bool> {
    let res = client
        .post("https://prod.live.maniaplanet.com/login/oauth2/access_token")
        .form(&MPAccessTokenBody {
            grant_type: "authorization_code",
            client_id: &crate::env().mp_client_id,
            client_secret: &crate::env().mp_client_secret,
            code,
            redirect_uri,
        })
        .send()
        .await
        .with_api_err()?
        .json()
        .await
        .with_api_err()?;

    let access_token = match res {
        MPAccessTokenResponse::AccessToken { access_token } => access_token,
        MPAccessTokenResponse::Error(err) => return Err(RecordsErrorKind::AccessTokenErr(err)),
    };

    check_mp_token(client, login, access_token).await
}

#[cfg(auth)]
async fn check_mp_token(client: &Client, login: &str, token: String) -> RecordsResult<bool> {
    let res = client
        .get("https://prod.live.maniaplanet.com/webservices/me")
        .header("Accept", "application/json")
        .bearer_auth(token)
        .send()
        .await
        .with_api_err()?;
    let MPServerRes { res_login } = match res.status() {
        StatusCode::OK => res.json().await.with_api_err()?,
        _ => return Ok(false),
    };

    Ok(res_login.to_lowercase() == login.to_lowercase())
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
    let conn = DbConn::from(db.mysql_pool);
    let mut redis_conn = db.redis_pool.get().await.with_api_err().fit(req_id)?;
    let res = finished_at(
        &conn,
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

#[cfg(auth)]
#[derive(Deserialize)]
pub struct GetTokenBody {
    login: String,
    state: String,
    redirect_uri: String,
}

#[derive(Serialize)]
struct GetTokenResponse {
    token: String,
}

// Handler used when the `auth` feature is disabled.
// This is used for older versions of the game that still rely on the `/player/get_token` route.
#[cfg(not(auth))]
async fn get_token() -> RecordsResponse<impl Responder> {
    json(GetTokenResponse {
        token: "if you see this gg".to_owned(),
    })
}

#[cfg(auth)]
async fn get_token(
    _: ApiAvailable,
    req_id: RequestId,
    db: Res<Database>,
    Res(client): Res<Client>,
    state: Data<AuthState>,
    Json(body): Json<GetTokenBody>,
) -> RecordsResponse<impl Responder> {
    // retrieve access_token from browser redirection
    let (tx, rx) = state
        .connect_with_browser(body.state.clone())
        .await
        .fit(req_id)?;
    let code = match timeout(TIMEOUT, rx).await {
        Ok(Ok(Message::MPCode(access_token))) => access_token,
        _ => {
            tracing::event!(
                Level::WARN,
                "Token state `{}` timed out, removing it",
                body.state.clone()
            );
            state.remove_state(body.state).await;
            return Err(RecordsErrorKind::Timeout(TIMEOUT)).fit(req_id);
        }
    };

    let err_msg = "/get_token rx should not be dropped at this point";

    // check access_token and generate new token for player ...
    match test_access_token(&client, &body.login, &code, &body.redirect_uri).await {
        Ok(true) => (),
        Ok(false) => {
            tx.send(Message::InvalidMPCode).expect(err_msg);
            return Err(RecordsErrorKind::InvalidMPCode).fit(req_id);
        }
        Err(RecordsErrorKind::AccessTokenErr(err)) => {
            tx.send(Message::AccessTokenErr(err.clone()))
                .expect(err_msg);
            return Err(RecordsErrorKind::AccessTokenErr(err)).fit(req_id);
        }
        err => {
            let _ = err.fit(req_id)?;
        }
    }

    let (mp_token, web_token) = auth::gen_token_for(&db, &body.login).await.fit(req_id)?;
    tx.send(Message::Ok(WebToken {
        login: body.login,
        token: web_token,
    }))
    .expect(err_msg);

    json(GetTokenResponse { token: mp_token })
}

#[cfg(not(auth))]
#[inline(always)]
async fn post_give_token() -> RecordsResponse<impl Responder> {
    Ok(HttpResponse::Ok().finish())
}

#[cfg(auth)]
#[derive(Deserialize)]
pub struct GiveTokenBody {
    code: String,
    state: String,
}

#[cfg(auth)]
async fn post_give_token(
    req_id: RequestId,
    session: Session,
    state: Data<AuthState>,
    Json(body): Json<GiveTokenBody>,
) -> RecordsResponse<impl Responder> {
    let web_token = state
        .browser_connected_for(body.state, body.code)
        .await
        .fit(req_id)?;
    session
        .insert(WEB_TOKEN_SESS_KEY, web_token)
        .expect("unable to insert session web token");
    Ok(HttpResponse::Ok().finish())
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
