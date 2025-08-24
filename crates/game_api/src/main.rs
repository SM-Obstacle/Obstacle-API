//! The ShootMania Obstacle API program.
//!
//! The program also includes a [library](game_api_lib). Overall, it uses the [`records_lib`] crate
//! as a main dependency.

use actix_cors::Cors;
use actix_session::{
    SessionMiddleware,
    config::{CookieContentSecurity, PersistentSession},
    storage::CookieSessionStore,
};
use actix_web::{
    App, HttpServer, Responder,
    cookie::{Key, time::Duration as CookieDuration},
    web::{self, Data},
};
use anyhow::Context;
use game_api_lib::{
    AuthState, FitRequestId, RecordsErrorKind, RecordsResponse, api_route, graphql_route,
};
use migration::MigratorTrait;
use records_lib::Database;
use reqwest::Client;
use tracing::level_filters::LevelFilter;
use tracing_actix_web::{DefaultRootSpanBuilder, RequestId, RootSpanBuilder, TracingLogger};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

struct CustomRootSpanBuilder;

impl RootSpanBuilder for CustomRootSpanBuilder {
    fn on_request_start(request: &actix_web::dev::ServiceRequest) -> tracing::Span {
        #[cfg_attr(
            all(not(feature = "mysql"), not(feature = "postgres")),
            allow(unused_variables)
        )]
        let db = request.app_data::<Database>().unwrap();
        let pool_size = {
            #[allow(unreachable_patterns)]
            match () {
                #[cfg(feature = "mysql")]
                _ => db.sql_conn.get_mysql_connection_pool().size(),
                #[cfg(feature = "postgres")]
                _ => db.sql_conn.get_postgres_connection_pool().size(),
                _ => 0,
            }
        };
        let pool_num_idle = {
            #[allow(unreachable_patterns)]
            match () {
                #[cfg(feature = "mysql")]
                _ => db.sql_conn.get_mysql_connection_pool().num_idle(),
                #[cfg(feature = "postgres")]
                _ => db.sql_conn.get_postgres_connection_pool().num_idle(),
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

/// The actix route handler for the Not Found response.
async fn not_found(req_id: RequestId) -> RecordsResponse<impl Responder> {
    Err::<String, _>(RecordsErrorKind::EndpointNotFound).fit(req_id)
}

/// The main entry point.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    let env = game_api_lib::init_env()?;
    #[cfg(feature = "request_filter")]
    request_filter::init_wh_url(env.used_once.wh_invalid_req_url)
        .unwrap_or_else(|_| panic!("Invalid request WH URL isn't supposed to be set twice"));

    let db =
        Database::from_db_url(env.db_env.db_url.db_url, env.db_env.redis_url.redis_url).await?;

    migration::Migrator::up(&db.sql_conn, None).await?;

    let client = Client::new();

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let max_connections = {
        #[allow(unreachable_patterns)]
        match () {
            #[cfg(feature = "mysql")]
            _ => db
                .sql_conn
                .get_mysql_connection_pool()
                .options()
                .get_max_connections(),
            #[cfg(feature = "postgres")]
            _ => db
                .sql_conn
                .get_postgres_connection_pool()
                .options()
                .get_max_connections(),
            _ => 0,
        }
    };

    tracing::info!("Using max connections: {max_connections}");

    let auth_state = Data::new(AuthState::default());

    let sess_key = Key::from(env.used_once.sess_key.as_bytes());
    drop(env.used_once.sess_key);

    HttpServer::new(move || {
        let cors = Cors::default()
            .supports_credentials()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec!["accept", "content-type"])
            .max_age(3600);
        #[cfg(debug_assertions)]
        let cors = cors.allow_any_origin();
        #[cfg(not(debug_assertions))]
        let cors = cors.allowed_origin(&game_api_lib::env().host);

        App::new()
            .wrap(cors)
            .wrap(TracingLogger::<CustomRootSpanBuilder>::new())
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), sess_key.clone())
                    .cookie_secure(cfg!(not(debug_assertions)))
                    .cookie_content_security(CookieContentSecurity::Private)
                    .session_lifecycle(PersistentSession::default().session_ttl(
                        CookieDuration::seconds(game_api_lib::env().auth_token_ttl as i64),
                    ))
                    .build(),
            )
            .app_data(auth_state.clone())
            .app_data(client.clone())
            .app_data(db.clone())
            .service(graphql_route(db.clone(), client.clone()))
            .service(api_route())
            .default_service(web::to(not_found))
    })
    .bind(("0.0.0.0", game_api_lib::env().port))
    .context("Cannot bind 0.0.0.0 address")?
    .run()
    .await
    .context("Cannot create actix-web server")?;

    Ok(())
}
