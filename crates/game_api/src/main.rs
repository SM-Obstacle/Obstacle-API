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
    App, HttpServer,
    cookie::{Key, time::Duration as CookieDuration},
    middleware,
};
use anyhow::Context;
use game_api_lib::configure;
use migration::MigratorTrait;
use mkenv::prelude::*;
use records_lib::Database;
use tracing::level_filters::LevelFilter;
use tracing_actix_web::TracingLogger;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

/// The main entry point.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    game_api_lib::init_env()?;
    #[cfg(feature = "request_filter")]
    request_filter::init_wh_url(game_api_lib::env().wh_invalid_req_url.get()).map_err(|_| {
        game_api_lib::internal!("Invalid request WH URL isn't supposed to be set twice")
    })?;

    let db = Database::from_db_url(
        game_api_lib::env().db_env.db_url.db_url.get(),
        game_api_lib::env().db_env.redis_url.redis_url.get(),
    )
    .await
    .context("Cannot initialize database connection")?;

    migration::Migrator::up(&db.sql_conn, None)
        .await
        .context("Cannot migrate")?;

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .try_init()
        .map_err(anyhow::Error::msg)
        .context("Cannot initialize trace subscriber")?;

    let max_connections = {
        #[allow(unreachable_patterns)]
        match db.sql_conn {
            #[cfg(feature = "mysql")]
            sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(_) => db
                .sql_conn
                .get_mysql_connection_pool()
                .options()
                .get_max_connections(),
            #[cfg(feature = "postgres")]
            sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(_) => db
                .sql_conn
                .get_postgres_connection_pool()
                .options()
                .get_max_connections(),
            _ => 0,
        }
    };

    tracing::info!("Using max connections: {max_connections}");

    let sess_key = Key::from(game_api_lib::env().dynamic.sess_key.get().as_bytes());

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
            .wrap(middleware::from_fn(configure::mask_internal_errors))
            .wrap(middleware::from_fn(configure::fit_request_id))
            .wrap(TracingLogger::<configure::RootSpanBuilder>::new())
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), sess_key.clone())
                    .cookie_secure(cfg!(not(debug_assertions)))
                    .cookie_content_security(CookieContentSecurity::Private)
                    .session_lifecycle(PersistentSession::default().session_ttl(
                        CookieDuration::seconds(game_api_lib::env().auth_token_ttl.get() as i64),
                    ))
                    .build(),
            )
            .configure(|cfg| configure::configure(cfg, db.clone()))
    })
    .bind(("0.0.0.0", game_api_lib::env().port.get()))
    .context("Cannot bind address")?
    .run()
    .await
    .context("Cannot run web server")?;

    Ok(())
}
