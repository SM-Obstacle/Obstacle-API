use actix_cors::Cors;
use actix_web::{
    web::{self, Data},
    App, HttpResponse, HttpServer,
};
use actix_web_grants::GrantsMiddleware;
use deadpool::Runtime;
use game_api::{
    api_route, auth_extractor, graphql_route, AuthState, Database, RecordsResult, UPDATE_RATE,
};
use sqlx::mysql;
use std::time::Duration;
use tokio::time::interval;
use tracing_actix_web::TracingLogger;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> RecordsResult<()> {
    // Filter traces based on the RUST_LOG env var, or, if it's not set,
    // default to show the output of the example.
    let filter = std::env::var("RECORDS_API_LOG")
        .unwrap_or_else(|_| "tracing=info,warp=info,game_api=info".to_owned());

    #[cfg(feature = "localhost_test")]
    let mut port = 3001u16;
    #[cfg(not(feature = "localhost_test"))]
    let mut port = 3000u16;
    if let Ok(s) = std::env::var("RECORDS_API_PORT") {
        if let Ok(env_port) = s.parse::<u16>() {
            port = env_port;
        }
    };

    let mysql_pool = mysql::MySqlPoolOptions::new().acquire_timeout(Duration::new(10, 0));
    #[cfg(feature = "localhost_test")]
    let mysql_pool = mysql_pool
        .connect("mysql://records_api:api@localhost/obs_records")
        .await?;
    #[cfg(not(feature = "localhost_test"))]
    let mysql_pool = mysql_pool
        .connect("mysql://root:root@localhost/obstacle_records")
        .await?;

    let redis_pool = {
        let cfg = deadpool_redis::Config {
            #[cfg(feature = "localhost_test")]
            url: Some("redis://127.0.0.1:6379/".to_string()),
            #[cfg(not(feature = "localhost_test"))]
            url: Some("redis://10.0.0.1/".to_string()),
            // url: Some("redis://localhost/".to_string()),
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
    let auth_update = auth_state.clone();
    tokio::spawn(async move {
        let mut int = interval(UPDATE_RATE);
        int.tick().await;
        loop {
            int.tick().await;
            auth_update.update().await;
        }
    });

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec!["accept", "content-type"])
            .max_age(3600);
        #[cfg(feature = "localhost_test")]
        let cors = cors.allow_any_origin();
        #[cfg(not(feature = "localhost_test"))]
        let cors = cors.allowed_origin("https://www.obstacle.ovh");

        App::new()
            .wrap(cors)
            .wrap(TracingLogger::default())
            .app_data(auth_state.clone())
            .app_data(Data::new(db.clone()))
            .wrap(GrantsMiddleware::with_extractor(auth_extractor))
            .service(graphql_route(db.clone()))
            .service(api_route())
            .default_service(web::to(|| async {
                HttpResponse::NotFound().body("Not found")
            }))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await?;

    Ok(())
}
