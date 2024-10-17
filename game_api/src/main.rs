//! The ShootMania Obstacle API program.
//!
//! The program also includes a [library](game_api_lib). Overall, it uses the [`records_lib`] crate
//! as a main dependency.

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_session::{
    config::{CookieContentSecurity, PersistentSession},
    storage::CookieSessionStore,
    SessionMiddleware,
};
use actix_web::{
    cookie::{time::Duration as CookieDuration, Key},
    web::{self, Data},
    App, HttpServer, Responder,
};
use anyhow::Context;
use game_api_lib::{
    api_route, graphql_route, poolsize_mw::ShowPoolSize, AuthState, FinishLocker, FitRequestId, RecordsErrorKind, RecordsResponse
};
use records_lib::{get_mysql_pool, get_redis_pool, Database};
use reqwest::Client;
use tracing_actix_web::{RequestId, TracingLogger};
use tracing_subscriber::fmt::format::FmtSpan;

/// The actix route handler for the Not Found response.
async fn not_found(req_id: RequestId) -> RecordsResponse<impl Responder> {
    Err::<String, _>(RecordsErrorKind::EndpointNotFound).fit(req_id)
}

/// The main entry point.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;
    let env = game_api_lib::init_env()?;

    let mysql_pool = get_mysql_pool(env.db_env.db_url.db_url)
        .await
        .context("Cannot create MySQL pool")?;
    let redis_pool =
        get_redis_pool(env.db_env.redis_url.redis_url).context("Cannot create Redis pool")?;

    let db = Database {
        mysql_pool,
        redis_pool,
    };

    let client = Client::new();

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    tracing::info!(
        "Using max connections: {}",
        db.mysql_pool.options().get_max_connections()
    );

    let auth_state = Data::new(AuthState::default());

    let sess_key = Key::from(env.used_once.sess_key.as_bytes());
    drop(env.used_once.sess_key);

    let governor_conf = GovernorConfigBuilder::default()
        .const_burst_size(32)
        .finish()
        .unwrap();

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
            .wrap(Governor::new(&governor_conf))
            .wrap(cors)
            .wrap(TracingLogger::default())
            .wrap(ShowPoolSize::default())
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
            .app_data(FinishLocker::default())
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
