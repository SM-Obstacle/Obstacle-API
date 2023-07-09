use actix_cors::Cors;
use actix_session::{
    config::{CookieContentSecurity, PersistentSession},
    storage::CookieSessionStore,
    SessionMiddleware,
};
use actix_web::{
    cookie::{time::Duration as CookieDuration, Key},
    web::{self, Data},
    App, HttpResponse, HttpServer,
};
use deadpool::Runtime;
use game_api::{
    api_route, get_tokens_ttl, graphql_route, read_env_var_file, AuthState, Database, RecordsResult, RecordsError,
};
#[cfg(not(feature = "localhost_test"))]
use game_api::{get_env_var, get_env_var_as};
use sqlx::mysql;
use std::env::var;
use std::time::Duration;
use tracing_actix_web::TracingLogger;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> RecordsResult<()> {
    // Filter traces based on the RUST_LOG env var, or, if it's not set,
    // default to show the output of the example.
    let filter = var("RECORDS_API_LOG")
        .unwrap_or_else(|_| "tracing=info,warp=info,game_api=info".to_owned());

    #[cfg(feature = "localhost_test")]
    let port = 3001;
    #[cfg(not(feature = "localhost_test"))]
    let port = get_env_var_as("RECORDS_API_PORT");

    let mysql_pool = mysql::MySqlPoolOptions::new().acquire_timeout(Duration::new(10, 0));
    #[cfg(feature = "localhost_test")]
    let mysql_pool = mysql_pool
        .connect("mysql://records_api:api@localhost/obs_records")
        .await?;
    #[cfg(not(feature = "localhost_test"))]
    let mysql_pool = mysql_pool
        .connect(&read_env_var_file("DATABASE_URL"))
        .await?;

    let redis_pool = {
        let cfg = deadpool_redis::Config {
            #[cfg(feature = "localhost_test")]
            url: Some("redis://127.0.0.1:6379/".to_string()),
            #[cfg(not(feature = "localhost_test"))]
            url: Some(get_env_var("REDIS_URL")),
            connection: None,
            pool: None,
        };
        cfg.create_pool(Some(Runtime::Tokio1)).unwrap()
    };

    let db = Database {
        mysql_pool,
        redis_pool,
    };

    // Configure the default `tracing` subscriber.
    // The `fmt` subscriber from the `tracing-subscriber` crate logs `tracing`
    // events to stdout. Other subscribers are available for integrating with
    // distributed tracing systems such as OpenTelemetry.
    tracing_subscriber::fmt()
        // Use the filter we built above to determine which traces to record.
        .with_env_filter(filter)
        // Record an event when each span closes. This can be used to time our
        // routes' durations!
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let auth_state = Data::new(AuthState::default());

    #[cfg(not(feature = "localhost_test"))]
    let localhost_origin = get_env_var("RECORDS_API_HOST");

    let sess_key = Key::from(read_env_var_file("RECORDS_API_SESSION_KEY_FILE").as_bytes());

    HttpServer::new(move || {
        let cors = Cors::default()
            .supports_credentials()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec!["accept", "content-type"])
            .max_age(3600);
        #[cfg(feature = "localhost_test")]
        let cors = cors.allow_any_origin();
        #[cfg(not(feature = "localhost_test"))]
        let cors = cors
            .allowed_origin("https://www.obstacle.ovh")
            .allowed_origin(&localhost_origin);

        App::new()
            .wrap(cors)
            .wrap(TracingLogger::default())
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), sess_key.clone())
                    .cookie_secure(!cfg!(feature = "localhost_test"))
                    .cookie_content_security(CookieContentSecurity::Private)
                    .session_lifecycle(
                        PersistentSession::default()
                            .session_ttl(CookieDuration::seconds(get_tokens_ttl() as i64)),
                    )
                    .build(),
            )
            .app_data(auth_state.clone())
            .app_data(Data::new(db.clone()))
            .service(graphql_route(db.clone()))
            .service(api_route())
            .default_service(web::to(|| async {
                Err::<HttpResponse, _>(RecordsError::EndpointNotFound)
            }))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await?;

    Ok(())
}
