//! Module used to serve the routes mainly used by the Obstacle gamemode. Each submodule is
//! specific for a route segment.

use std::fmt;

use actix_web::body::BoxBody;
use actix_web::dev::{ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::web::{JsonConfig, Query};
use actix_web::{HttpResponse, Scope, web};
use records_lib::{Database, acquire};
use serde::Serialize;
use tracing_actix_web::RequestId;

#[cfg(auth)]
use self::admin::admin_scope;
use self::event::event_scope;
use self::map::map_scope;
use self::player::player_scope;
use self::staggered::staggered_scope;
use crate::utils::{self, ApiStatus, get_api_status, json};
use crate::{FitRequestId as _, ModeVersion, RecordsResponse, RecordsResultExt, Res};
use actix_web::Responder;
use dsc_webhook::{WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};
#[cfg(feature = "request_filter")]
use request_filter::{FlagFalseRequest, InGameFilter};

#[cfg(auth)]
pub mod admin;
pub mod event;
pub mod map;
pub mod player;

mod overview;
mod pb;
mod player_finished;
mod staggered;

pub fn api_route() -> Scope<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<BoxBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let scope = web::scope("");

    #[cfg(feature = "request_filter")]
    let scope = scope.wrap(FlagFalseRequest::<InGameFilter>::default());

    #[cfg(auth)]
    let scope = scope.service(admin_scope());

    let scope = scope
        .route("/latestnews_image", web::get().to(latestnews_image))
        .route("/overview", web::get().to(overview))
        .route("/report", web::post().to(report_error))
        .service(staggered_scope())
        .service(player_scope())
        .service(map_scope())
        .service(event_scope());

    let json_config = JsonConfig::default().limit(1024 * 16);

    web::scope("")
        .app_data(json_config)
        .route("/info", web::get().to(info))
        .service(scope)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum HttpMethod {
    Get,
    Post,
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpMethod::Get => f.write_str("GET"),
            HttpMethod::Post => f.write_str("POST"),
        }
    }
}

#[derive(serde::Deserialize)]
struct ReportErrorBody {
    method: HttpMethod,
    route: String,
    body: String,
    player_login: String,
    status_code: i32,
    error: String,
}

async fn report_error(
    Res(client): Res<reqwest::Client>,
    req_id: RequestId,
    web::Json(body): web::Json<ReportErrorBody>,
    mode_vers: ModeVersion,
) -> RecordsResponse<impl Responder> {
    let mut fields = vec![
        WebhookBodyEmbedField {
            name: "HTTP method".to_owned(),
            value: body.method.to_string(),
            inline: None,
        },
        WebhookBodyEmbedField {
            name: "API route".to_owned(),
            value: format!("`{}`", body.route),
            inline: None,
        },
    ];

    if !body.player_login.is_empty() {
        fields.push(WebhookBodyEmbedField {
            name: "Player login".to_owned(),
            value: format!("`{}`", body.player_login),
            inline: None,
        });
    }

    if !body.body.is_empty() {
        fields.push(WebhookBodyEmbedField {
            name: "Request body".to_owned(),
            value: format!("```{}```", body.body),
            inline: None,
        });
    }

    client
        .post(&crate::env().wh_report_url)
        .json(&WebhookBody {
            content: format!("Error reported (mode version: {mode_vers})"),
            embeds: vec![
                WebhookBodyEmbed {
                    title: "Error".to_owned(),
                    description: Some(format!("```{}```", body.error)),
                    color: 5814783,
                    fields: Some(vec![WebhookBodyEmbedField {
                        name: "Status code".to_owned(),
                        value: body.status_code.to_string(),
                        inline: None,
                    }]),
                    url: None,
                },
                WebhookBodyEmbed {
                    title: "Context".to_owned(),
                    description: None,
                    color: 5814783,
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

#[derive(Serialize, sqlx::FromRow)]
struct LatestnewsImageResponse {
    img_url: String,
    link: String,
}

async fn latestnews_image(req_id: RequestId, db: Res<Database>) -> RecordsResponse<impl Responder> {
    let res: LatestnewsImageResponse = sqlx::query_as("select * from latestnews_image")
        .fetch_one(&db.mysql_pool)
        .await
        .with_api_err()
        .fit(req_id)?;
    json(res)
}

#[derive(Serialize)]
struct InfoResponse {
    service_name: &'static str,
    contacts: &'static str,
    api_version: &'static str,
    status: ApiStatus,
}

async fn info(req_id: RequestId, db: Res<Database>) -> RecordsResponse<impl Responder> {
    let api_version = env!("CARGO_PKG_VERSION");
    let status = get_api_status(&db).await.fit(req_id)?;

    json(InfoResponse {
        service_name: "Obstacle Records API",
        contacts: "Discord: @ahmadbky, @miltant",
        api_version,
        status,
    })
}

async fn overview(
    req_id: RequestId,
    db: Res<Database>,
    Query(query): overview::OverviewReq,
) -> RecordsResponse<impl Responder> {
    let conn = acquire!(db.with_api_err().fit(req_id)?);

    let map = records_lib::must::have_map(conn.mysql_conn, &query.map_uid)
        .await
        .with_api_err()
        .fit(req_id)?;

    let res = overview::overview(conn, &query.login, &map, Default::default())
        .await
        .fit(req_id)?;
    utils::json(res)
}
