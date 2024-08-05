use actix_session::Session;
use actix_web::{
    web::{self, Data, Json, Query},
    HttpResponse, Responder, Scope,
};
use futures::TryStreamExt;
use records_lib::{
    event::{self, OptEvent},
    models::Banishment,
    must, Database,
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, MySqlConnection};
use tokio::time::timeout;
use tracing::Level;
use tracing_actix_web::RequestId;

use crate::{
    auth::{
        self, privilege, ApiAvailable, AuthHeader, AuthState, MPAuthGuard, Message, WebToken,
        TIMEOUT, WEB_TOKEN_SESS_KEY,
    },
    utils::json,
    AccessTokenErr, FitRequestId, RecordsErrorKind, RecordsResponse, RecordsResult,
    RecordsResultExt, Res,
};

use super::{pb, player_finished as pf};

pub fn player_scope() -> Scope {
    web::scope("/player")
        .route("/update", web::post().to(update))
        .route("/finished", web::post().to(finished))
        .route("/get_token", web::post().to(get_token))
        .route("/give_token", web::post().to(post_give_token))
        .route("/pb", web::get().to(pb))
        .route("/times", web::post().to(times))
        .route("/info", web::get().to(info))
        .route("/report_error", web::post().to(report_error))
        .route("/ac", web::post().to(ac))
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

pub async fn check_banned(
    db: &mut MySqlConnection,
    player_id: u32,
) -> Result<Option<Banishment>, RecordsErrorKind> {
    let r = sqlx::query_as("SELECT * FROM current_bans WHERE player_id = ?")
        .bind(player_id)
        .fetch_optional(db)
        .await
        .with_api_err()?;
    Ok(r)
}

#[derive(Serialize)]
struct MPAccessTokenBody<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MPAccessTokenResponse {
    AccessToken { access_token: String },
    Error(AccessTokenErr),
}

#[derive(Deserialize, Debug)]
struct MPServerRes {
    #[serde(alias = "login")]
    res_login: String,
}

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

pub async fn finished_at(
    req_id: RequestId,
    login: String,
    db: Res<Database>,
    body: pf::HasFinishedBody,
    at: chrono::NaiveDateTime,
) -> RecordsResponse<impl Responder> {
    let mut conn = db.acquire().await.with_api_err().fit(req_id)?;
    let res = pf::finished(
        login,
        &mut conn,
        body.into_params(None),
        Default::default(),
        at,
    )
    .await
    .fit(req_id)?
    .res;
    json(res)
}

#[inline(always)]
async fn finished(
    _: ApiAvailable,
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    body: pf::PlayerFinishedBody,
) -> RecordsResponse<impl Responder> {
    finished_at(req_id, login, db, body.0, chrono::Utc::now().naive_utc()).await
}

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

pub async fn get_token(
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
            return Err(RecordsErrorKind::Timeout).fit(req_id);
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

#[derive(Deserialize)]
pub struct GiveTokenBody {
    code: String,
    state: String,
}

pub async fn post_give_token(
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

    match edition {
        Some((event_id, edition_id, _)) if single_edition => {
            let (event, edition) =
                must::have_event_edition_from_ids(&mut mysql_conn, event_id, edition_id)
                    .await
                    .with_api_err()
                    .fit(req_id)?;
            pb::pb(login, req_id, db, body, OptEvent::new(&event, &edition)).await
        }
        _ => pb::pb(login, req_id, db, body, Default::default()).await,
    }
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

    let query = format!(
        "SELECT m.game_id AS map_uid, MIN(r.time) AS time
        FROM maps m
        INNER JOIN records r ON r.map_id = m.id
        WHERE r.record_player_id = ? AND m.game_id IN ({})
        GROUP BY m.id",
        body.maps_uids
            .iter()
            .map(|_| "?".to_owned())
            .collect::<Vec<_>>()
            .join(",")
    );

    let mut query = sqlx::query_as::<_, TimesResponseItem>(&query).bind(player.id);

    for map_uid in body.maps_uids {
        query = query.bind(map_uid);
    }

    let result = query
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

#[derive(Serialize)]
struct WebhookBodyEmbedField {
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline: Option<bool>,
}

#[derive(Serialize)]
struct WebhookBodyEmbed {
    title: String,
    description: Option<String>,
    color: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    fields: Option<Vec<WebhookBodyEmbedField>>,
}

#[derive(Serialize)]
struct WebhookBody {
    content: String,
    embeds: Vec<WebhookBodyEmbed>,
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
