use sqlx::mysql;
use std::convert::Infallible;
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;
use warp::{http::StatusCode, Filter, Rejection, Reply};

pub mod graphql;
pub mod http;
pub mod xml;

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    // Send bad request if it is an error from graphql
    if let Some(async_graphql_warp::BadRequest(_)) = err.find() {
        Ok(warp::reply::with_status(
            "Bad graphql request".to_string(),
            StatusCode::BAD_REQUEST,
        ))
    } else if let Some(err) = err.find::<records_lib::RecordsError>() {
        Ok(warp::reply::with_status(
            err.to_string(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ))
    } else {
        Ok(warp::reply::with_status(
            "Not found".to_string(),
            StatusCode::NOT_FOUND,
        ))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mysql_pool = mysql::MySqlPoolOptions::new()
        .connect_timeout(Duration::new(10, 0))
        // .connect("mysql://vincent:vincent@10.0.0.1/test2")
        .connect("mysql://root:root@localhost/test2")
        .await?;

    let redis_pool = {
        let cfg = deadpool_redis::Config {
            url: Some("redis://10.0.0.1/".to_string()),
            connection: None,
            pool: None,
        };
        dbg!(&cfg);
        cfg.create_pool().unwrap()
    };

    let db = records_lib::Database {
        mysql_pool,
        redis_pool,
    };

    println!(
        "{:?}",
        records_lib::update_player(&db, &"adelete".to_string(), None).await?
    );

    // Filter traces based on the RUST_LOG env var, or, if it's not set,
    // default to show the output of the example.
    let filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "tracing=info,warp=debug,game_api=trace".to_owned());

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

    let routes = http::warp_routes(db.clone())
        .or(graphql::warp_routes(db))
        .recover(handle_rejection)
        .with(warp::trace::request());

    warp::serve(routes).run(([0, 0, 0, 0], 8000)).await;
    Ok(())
}
