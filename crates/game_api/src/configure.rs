use actix_web::{Responder, web};
use records_lib::Database;
use tracing_actix_web::{DefaultRootSpanBuilder, RequestId, RootSpanBuilder};

use crate::{FitRequestId as _, RecordsErrorKind, RecordsResponse};

/// The actix route handler for the Not Found response.
async fn not_found(req_id: RequestId) -> RecordsResponse<impl Responder> {
    Err::<String, _>(RecordsErrorKind::EndpointNotFound).fit(req_id)
}

pub struct CustomRootSpanBuilder;

impl RootSpanBuilder for CustomRootSpanBuilder {
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
