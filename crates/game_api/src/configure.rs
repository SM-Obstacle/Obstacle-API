use actix_http::StatusCode;
use actix_web::{
    Error, Responder,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
    web,
};
use dsc_webhook::{FormattedRequestHead, WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};
use records_lib::Database;
use tracing_actix_web::{DefaultRootSpanBuilder, RequestId};

use crate::{RecordsErrorKind, RecordsErrorKindResponse, RecordsResult, Res, TracedError};

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

            let wh_msg = WebhookBody {
                content: "Got an internal error 🤕".to_owned(),
                embeds: vec![
                    WebhookBodyEmbed {
                        title: "Request".to_owned(),
                        description: None,
                        color: 5814783,
                        fields: Some(vec![WebhookBodyEmbedField {
                            name: "Head".to_owned(),
                            value: format!("```\n{}\n```", FormattedRequestHead::new(&head)),
                            inline: None,
                        }]),
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

            let req_send = client
                .post(&crate::env().wh_report_url)
                .json(&wh_msg)
                .send();

            tokio::task::spawn(req_send);

            let new_err = TracedError {
                error: RecordsErrorKind::Lib(records_lib::error::RecordsError::MaskedInternal)
                    .into(),
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

/// The actix route handler for the Not Found response.
async fn not_found() -> RecordsResult<impl Responder> {
    Err::<String, _>(RecordsErrorKind::EndpointNotFound)
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
        .app_data(db.clone())
        .service(crate::graphql_route(db.clone(), client))
        .service(crate::api_route())
        .default_service(web::to(not_found));
}
