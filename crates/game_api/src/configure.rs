use std::fmt;

use actix_http::StatusCode;
use actix_web::{
    Error, Responder,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
    web,
};
use dsc_webhook::{FormattedRequestHead, WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};
use itertools::{EitherOrBoth, Itertools as _};
use records_lib::{
    Database,
    error::RecordsError,
    pool::clone_dbconn,
    ranks::{DbLeaderboardItem, RankComputeError},
};
use reqwest::multipart::{Form, Part};
use tracing_actix_web::{DefaultRootSpanBuilder, RequestId};

use crate::{ApiErrorKind, RecordsErrorKindResponse, RecordsResult, Res, TracedError};

pub async fn fit_request_id(
    request_id: RequestId,
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    match next.call(req).await {
        Ok(mut res) => {
            let err = res
                .response_mut()
                .extensions_mut()
                .get_mut::<Option<RecordsErrorKindResponse>>()
                .and_then(Option::take);

            match err {
                Some(err) => Err(TracedError {
                    status_code: Some(res.status()),
                    error: Box::<dyn std::error::Error>::from(err.message).into(),
                    request_id,
                    r#type: Some(err.r#type),
                }
                .into()),
                None if res.status().is_client_error() || res.status().is_server_error() => {
                    let status = res.status();

                    let err_msg = match res.response().error() {
                        Some(err) => err.to_string(),
                        None => match actix_http::body::to_bytes(res.into_body()).await {
                            Ok(body) => String::from_utf8_lossy(&body).to_string(),
                            // TODO: handle error
                            Err(_) => "Unknown error".to_owned(),
                        },
                    };

                    Err(TracedError {
                        status_code: Some(status),
                        r#type: None,
                        error: Box::<dyn std::error::Error>::from(err_msg).into(),
                        request_id,
                    }
                    .into())
                }
                None => Ok(res),
            }
        }
        Err(error) => Err(TracedError {
            status_code: None,
            r#type: None,
            error,
            request_id,
        }
        .into()),
    }
}

pub async fn mask_internal_errors(
    request_id: RequestId,
    client: Res<reqwest::Client>,
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let head = req.head().clone();

    let res = next.call(req).await;
    let err = match res.as_ref() {
        Ok(res) if res.status().is_server_error() => res.response().error(),
        Ok(_) => None,
        Err(e) => Some(e),
    };

    match err {
        Some(err) => {
            tracing::error!(
                "[{request_id}] Got an internal error when processing a request: {err}"
            );

            send_internal_err_msg_detached(client.0, head, request_id, err);

            let new_err = TracedError {
                error: ApiErrorKind::Lib(records_lib::error::RecordsError::MaskedInternal).into(),
                request_id,
                status_code: Some(StatusCode::INTERNAL_SERVER_ERROR),
                r#type: None,
            }
            .into();

            match res {
                Ok(res) => {
                    let (req, _) = res.into_parts();
                    Ok(ServiceResponse::from_err(new_err, req))
                }
                Err(_) => Err(new_err),
            }
        }
        None => res.map(|res| res.map_into_boxed_body()),
    }
}

pub(crate) fn send_internal_err_msg_detached<E>(
    client: reqwest::Client,
    head: actix_http::RequestHead,
    request_id: RequestId,
    err: E,
) where
    E: fmt::Display + fmt::Debug,
{
    let wh_msg = WebhookBody {
        content: "Got an internal error 🤕".to_owned(),
        embeds: vec![
            WebhookBodyEmbed {
                title: "Request".to_owned(),
                description: None,
                color: 5814783,
                fields: Some(vec![
                    WebhookBodyEmbedField {
                        name: "Head".to_owned(),
                        value: format!("```\n{}\n```", FormattedRequestHead::new(&head)),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Request ID".to_owned(),
                        value: format!("```\n{}\n```", request_id),
                        inline: None,
                    },
                ]),
                url: None,
            },
            WebhookBodyEmbed {
                title: "Error message".to_owned(),
                description: None,
                color: 5814783,
                fields: Some(vec![
                    WebhookBodyEmbedField {
                        name: "Raw (display)".to_owned(),
                        value: format!("```\n{err}\n```"),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Raw (debug)".to_owned(),
                        value: format!("```\n{err:?}\n```"),
                        inline: None,
                    },
                ]),
                url: None,
            },
        ],
    };

    tokio::task::spawn(async move {
        if let Err(e) = client
            .post(&crate::env().wh_report_url)
            .json(&wh_msg)
            .send()
            .await
        {
            tracing::error!("couldn't send internal error to webhook: {e}. body:\n{wh_msg:#?}");
        }
    });
}

pub async fn mask_rank_compute_error(
    request_id: RequestId,
    client: Res<reqwest::Client>,
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let res = next.call(req).await?;

    if let Some(err) = res.response().error()
        && let Some(err) = err.as_error::<ApiErrorKind>()
        && let ApiErrorKind::Lib(RecordsError::RankCompute(err)) = err
    {
        send_compute_err_msg_detached(client.0, request_id, err.clone());
    }

    Ok(res)
}

struct CsvLeaderboard {
    buf: String,
}

impl CsvLeaderboard {
    fn new(lb_len: usize) -> Self {
        const HEADER: &str = "player_id,time\n";
        // Consider each player ID having 5 digits on average, and each time having 6.
        // Add 2 for the comma and new line char
        let mut buf = String::with_capacity(const { HEADER.len() } + lb_len * (5 + 6 + 2));
        buf.push_str(HEADER);
        Self { buf }
    }

    fn add_row(&mut self, player_id: u32, time: i32) {
        use std::fmt::Write as _;
        writeln!(self.buf, "{player_id},{time}").expect("couldn't format row when building CSV");
    }
}

pub(crate) fn send_compute_err_msg_detached(
    client: reqwest::Client,
    request_id: RequestId,
    err: RankComputeError,
) {
    tokio::task::spawn(async move {
        let lb = err
            .raw_redis_lb()
            .as_chunks()
            .0
            .iter()
            .map(|[player_id, score]| (*player_id as u32, *score as i32))
            .zip_longest(err.sql_lb());

        let mut redis_table = CsvLeaderboard::new(lb.len());
        let mut sql_table = CsvLeaderboard::new(lb.len());

        for row in lb {
            match row {
                EitherOrBoth::Both(
                    (redis_player_id, redis_time),
                    DbLeaderboardItem {
                        record_player_id: sql_player_id,
                        time: sql_time,
                    },
                ) => {
                    redis_table.add_row(redis_player_id, redis_time);
                    sql_table.add_row(*sql_player_id, *sql_time);
                }
                EitherOrBoth::Left((redis_player_id, redis_time)) => {
                    redis_table.add_row(redis_player_id, redis_time);
                }
                EitherOrBoth::Right(DbLeaderboardItem {
                    record_player_id: sql_player_id,
                    time: sql_time,
                }) => {
                    sql_table.add_row(*sql_player_id, *sql_time);
                }
            }
        }

        let wh_msg = WebhookBody {
            content: "Rank compute error 💢".to_owned(),
            embeds: vec![WebhookBodyEmbed {
                title: "Error info".to_owned(),
                description: None,
                color: 5814783,
                fields: Some(vec![
                    WebhookBodyEmbedField {
                        name: "Request ID".to_owned(),
                        value: request_id.to_string(),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Player ID".to_owned(),
                        value: err.player_id().to_string(),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Map ID".to_owned(),
                        value: err.map_id().to_string(),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Time".to_owned(),
                        value: err.time().to_string(),
                        inline: None,
                    },
                    WebhookBodyEmbedField {
                        name: "Tested time".to_owned(),
                        value: match err.tested_time() {
                            Some(time) => time.to_string(),
                            None => "None".to_owned(),
                        },
                        inline: None,
                    },
                ]),
                url: None,
            }],
        };

        let wh_msg = match serde_json::to_string(&wh_msg) {
            Ok(out) => out,
            Err(_) => format!("{wh_msg:#?}"),
        };

        let send = client
            .post(&crate::env().wh_rank_compute_err)
            .multipart(
                Form::new()
                    .part(
                        "payload_json",
                        Part::bytes(wh_msg.clone().into_bytes())
                            .mime_str("application/json")
                            .unwrap(),
                    )
                    .part(
                        "files[0]",
                        Part::bytes(redis_table.buf.clone().into_bytes())
                            .file_name("redis_table.txt")
                            .mime_str("text/plain")
                            .unwrap(),
                    )
                    .part(
                        "files[1]",
                        Part::bytes(sql_table.buf.clone().into_bytes())
                            .file_name("sql_table.txt")
                            .mime_str("text/plain")
                            .unwrap(),
                    ),
            )
            .send()
            .await;

        if let Err(err) = send {
            tracing::error!(
                "couldn't send rank compute error to webhook: {err}. body:\n{wh_msg:#?}"
            );
        }
    });
}

/// The actix route handler for the Not Found response.
async fn not_found() -> RecordsResult<impl Responder> {
    Err::<String, _>(ApiErrorKind::EndpointNotFound)
}

pub struct RootSpanBuilder;

impl tracing_actix_web::RootSpanBuilder for RootSpanBuilder {
    fn on_request_start(request: &actix_web::dev::ServiceRequest) -> tracing::Span {
        #[cfg_attr(
            all(not(feature = "mysql"), not(feature = "postgres")),
            allow(unused_variables)
        )]
        let db = request.app_data::<Database>().unwrap();
        let pool_size = {
            #[allow(unreachable_patterns)]
            match db.sql_conn {
                #[cfg(feature = "mysql")]
                sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(_) => {
                    db.sql_conn.get_mysql_connection_pool().size()
                }
                #[cfg(feature = "postgres")]
                sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(_) => {
                    db.sql_conn.get_postgres_connection_pool().size()
                }
                _ => 0,
            }
        };
        let pool_num_idle = {
            #[allow(unreachable_patterns)]
            match db.sql_conn {
                #[cfg(feature = "mysql")]
                sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(_) => {
                    db.sql_conn.get_mysql_connection_pool().num_idle()
                }
                #[cfg(feature = "postgres")]
                sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(_) => {
                    db.sql_conn.get_postgres_connection_pool().num_idle()
                }
                _ => 0,
            }
        };

        tracing_actix_web::root_span!(
            request,
            pool_size = pool_size,
            pool_num_idle = pool_num_idle,
        )
    }

    fn on_request_end<B: actix_web::body::MessageBody>(
        span: tracing::Span,
        outcome: &Result<actix_web::dev::ServiceResponse<B>, actix_web::Error>,
    ) {
        DefaultRootSpanBuilder::on_request_end(span, outcome);
    }
}

pub fn configure(cfg: &mut web::ServiceConfig, db: Database) {
    let client = reqwest::Client::default();

    cfg.app_data(web::Data::new(crate::AuthState::default()))
        .app_data(client.clone())
        .app_data(clone_dbconn(&db.sql_conn))
        .app_data(db.redis_pool.clone())
        .app_data(db.clone())
        .service(crate::graphql_route(db.clone(), client))
        .service(crate::api_route())
        .default_service(web::to(not_found));
}
