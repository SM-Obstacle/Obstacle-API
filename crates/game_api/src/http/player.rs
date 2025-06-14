#[cfg(auth)]
use actix_session::Session;
#[cfg(auth)]
use actix_web::web::Data;
use actix_web::{
    HttpResponse, Responder, Scope,
    body::BoxBody,
    web::{self, Json, Query},
};
use futures::TryStreamExt;
use records_lib::{
    Database, DatabaseConnection, ModeVersion, TxnDatabaseConnection, acquire,
    event::{self},
    models::{self, Banishment},
    must,
    opt_event::OptEvent,
    player,
    transaction::{self, CanWrite, ReadWrite},
};
use reqwest::Client;
#[cfg(auth)]
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
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
    discord_webhook::{WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField},
    utils::{self, json},
};

#[cfg(feature = "request_filter")]
use crate::request_filter::{FlagFalseRequest, WebsiteFilter};

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

#[derive(Serialize, Deserialize, Clone, FromRow, Debug)]
pub struct PlayerInfoNetBody {
    pub login: String,
    pub name: String,
    pub zone_path: Option<String>,
}

async fn insert_player(db: &Database, body: &PlayerInfoNetBody) -> RecordsResult<u32> {
    let id = sqlx::query_scalar(
        "INSERT INTO players
        (login, name, join_date, zone_path, admins_note, role)
        VALUES (?, ?, SYSDATE(), ?, NULL, 0) RETURNING id",
    )
    .bind(&body.login)
    .bind(&body.name)
    .bind(&body.zone_path)
    .fetch_one(&db.mysql_pool)
    .await
    .with_api_err()?;

    Ok(id)
}

pub async fn get_or_insert(db: &Database, body: &PlayerInfoNetBody) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM players WHERE login = ?")
        .bind(&body.login)
        .fetch_optional(&db.mysql_pool)
        .await
        .with_api_err()?
    {
        return Ok(id);
    }

    insert_player(db, body).await
}

pub async fn update(
    _: ApiAvailable,
    req_id: RequestId,
    db: Res<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<PlayerInfoNetBody>,
) -> RecordsResponse<impl Responder> {
    match auth::check_auth_for(&db, &login, &token, privilege::PLAYER).await {
        Ok(id) => update_player(&db, id, body).await.fit(req_id)?,
        // At this point, if Redis has registered a token with the login, it means that
        // the player is not yet added to the Obstacle database but effectively
        // has a ManiaPlanet account
        Err(RecordsErrorKind::Lib(records_lib::error::RecordsError::PlayerNotFound(_))) => {
            let _ = insert_player(&db, &body).await.fit(req_id)?;
        }
        Err(e) => return Err(e).fit(req_id),
    }

    Ok(HttpResponse::Ok().finish())
}

pub async fn update_player(
    db: &Database,
    player_id: u32,
    body: PlayerInfoNetBody,
) -> RecordsResult<()> {
    sqlx::query("UPDATE players SET name = ?, zone_path = ? WHERE id = ?")
        .bind(body.name)
        .bind(body.zone_path)
        .bind(player_id)
        .execute(&db.mysql_pool)
        .await
        .with_api_err()?;

    Ok(())
}

pub async fn get_ban_during(
    db: &mut sqlx::MySqlConnection,
    player_id: u32,
    at: chrono::NaiveDateTime,
) -> RecordsResult<Option<Banishment>> {
    sqlx::query_as(
        "select * from banishments where player_id = ? and date_ban < ?
            and (duration is null or date_ban + duration > ?)",
    )
    .bind(player_id)
    .bind(at)
    .bind(at)
    .fetch_optional(db)
    .await
    .map_err(From::from)
}

pub async fn check_banned(
    db: &mut sqlx::MySqlConnection,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsErrorKind> {
    let r = sqlx::query_as("SELECT * FROM current_bans WHERE player_id = ?")
        .bind(player_id)
        .fetch_optional(db)
        .await
        .with_api_err()?;
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

async fn finished_impl<M: CanWrite>(
    conn: &mut TxnDatabaseConnection<'_, M>,
    params: ExpandedInsertRecordParams<'_>,
    player_login: &str,
    map: &models::Map,
) -> RecordsResult<pf::FinishedOutput> {
    let res = pf::finished(conn, params, player_login, map).await?;

    // If the record isn't in an event context, save the record to the events that have the map
    // and allow records saving without an event context.
    let editions = records_lib::event::get_editions_which_contain(conn.conn.mysql_conn, map.id)
        .try_collect::<Vec<_>>()
        .await
        .with_api_err()?;
    for (event_id, edition_id, original_map_id) in editions {
        super::event::insert_event_record(
            conn.conn.mysql_conn,
            res.record_id,
            event_id,
            edition_id,
        )
        .await?;

        let Some(original_map_id) = original_map_id else {
            continue;
        };

        // Get the previous time of the player on the original map, to check if it would be a PB or not.
        let previous_time = player::get_time_on_map(
            conn.conn.mysql_conn,
            res.player_id,
            original_map_id,
            Default::default(),
        )
        .await?;
        let is_pb = previous_time.is_none_or(|t| t > params.body.time);

        pf::insert_record(
            conn,
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
    let mut conn = acquire!(db.with_api_err().fit(req_id)?);
    let res = finished_at(&mut conn, req_id, mode_version, login, body, at).await?;
    Ok(res)
}

pub async fn finished_at(
    conn: &mut DatabaseConnection<'_>,
    req_id: RequestId,
    mode_version: Option<ModeVersion>,
    login: String,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder<Body = BoxBody> + use<>> {
    let map = must::have_map(conn.mysql_conn, &body.map_uid)
        .await
        .with_api_err()
        .fit(req_id)?;

    let res: pf::FinishedOutput =
        transaction::within(conn.mysql_conn, ReadWrite, async |mysql_conn, guard| {
            let params = ExpandedInsertRecordParams {
                body: &body.rest,
                at,
                event: Default::default(),
                mode_version,
            };

            finished_impl(
                &mut TxnDatabaseConnection::new(
                    guard,
                    DatabaseConnection {
                        mysql_conn,
                        redis_conn: conn.redis_conn,
                    },
                ),
                params,
                &login,
                &map,
            )
            .await
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
    db: Res<Database>,
    Query(body): pb::PbReq,
) -> RecordsResponse<impl Responder> {
    let mut mysql_conn = db.0.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let map = must::have_map(&mut mysql_conn, &body.map_uid)
        .await
        .with_api_err()
        .fit(req_id)?;

    let mut editions = event::get_editions_which_contain(&mut mysql_conn, map.id);
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
            let (event, edition) =
                must::have_event_edition_from_ids(&mut mysql_conn, event_id, edition_id)
                    .await
                    .with_api_err()
                    .fit(req_id)?;
            pb::pb(
                db.0.mysql_pool,
                &login,
                &body.map_uid,
                OptEvent::new(&event, &edition),
            )
            .await
        }
        _ => pb::pb(db.0.mysql_pool, &login, &body.map_uid, Default::default()).await,
    }
    .fit(req_id)?;

    utils::json(res)
}

#[derive(Deserialize)]
struct TimesBody {
    maps_uids: Vec<String>,
}

#[derive(Serialize, FromRow)]
struct TimesResponseItem {
    map_uid: String,
    time: i32,
}

async fn times(
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    Json(body): Json<TimesBody>,
) -> RecordsResponse<impl Responder> {
    let mut mysql_conn = db.0.mysql_pool.acquire().await.with_api_err().fit(req_id)?;

    let player = records_lib::must::have_player(&mut mysql_conn, &login)
        .await
        .fit(req_id)?;

    let mut query = sqlx::QueryBuilder::new(
        "SELECT m.game_id AS map_uid, MIN(r.time) AS time
        FROM maps m
        INNER JOIN records r ON r.map_id = m.id
        WHERE r.record_player_id = ",
    );
    let mut sep = query
        .push_bind(player.id)
        .push(" AND m.game_id IN (")
        .separated(", ");
    for uid in body.maps_uids {
        sep.push_bind(uid);
    }
    let result = query
        .push(") GROUP BY m.id")
        .build_query_as::<TimesResponseItem>()
        .fetch_all(&mut *mysql_conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(result)
}

#[derive(Deserialize)]
pub struct InfoBody {
    login: String,
}

#[derive(Serialize, FromRow)]
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
    db: Res<Database>,
    Query(body): Query<InfoBody>,
) -> RecordsResponse<impl Responder> {
    let Some(info) = sqlx::query_as::<_, InfoResponse>(
        "SELECT *, (SELECT role_name FROM role WHERE id = role) as role_name
        FROM players WHERE login = ?",
    )
    .bind(&body.login)
    .fetch_optional(&db.mysql_pool)
    .await
    .with_api_err()
    .fit(req_id)?
    else {
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
