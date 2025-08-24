use actix_http::Request;
use actix_web::{
    App, Error,
    body::MessageBody,
    dev::{Service, ServiceResponse},
    http::StatusCode,
    test,
};
use anyhow::Context;
use nom::AsBytes;
use records_lib::Database;
use sea_orm::DbBackend;
use tracing_actix_web::TracingLogger;

use crate::{configure, init_env};

#[derive(serde::Deserialize)]
struct ErrorResponse<'a> {
    #[allow(dead_code)]
    request_id: &'a str,
    r#type: i32,
    message: &'a str,
}

fn get_db(backend: DbBackend) -> anyhow::Result<Database> {
    match dotenvy::dotenv() {
        Err(err) if !err.not_found() => return Err(err).context("retrieving .env files"),
        _ => (),
    }

    let env = init_env()?;

    Ok(Database::from_mock_db(
        backend,
        env.db_env.redis_url.redis_url,
    )?)
}

async fn get_app(
    db: Database,
) -> impl Service<Request, Response = ServiceResponse<impl MessageBody>, Error = Error> {
    test::init_service(
        App::new()
            .wrap(TracingLogger::<configure::CustomRootSpanBuilder>::new())
            .configure(|cfg| configure::configure(cfg, db.clone())),
    )
    .await
}

#[tokio::test]
async fn test_not_found() -> anyhow::Result<()> {
    let db = get_db(DbBackend::MySql)?;
    let app = get_app(db).await;
    let req = test::TestRequest::get().uri("/").to_request();

    let resp = test::call_service(&app, req).await;
    let status_code = resp.status();

    let body = test::read_body(resp).await;
    let error: ErrorResponse = serde_json::from_slice(body.as_bytes())?;

    assert_eq!(status_code, StatusCode::NOT_FOUND);
    assert_eq!(error.r#type, 301);
    assert_eq!(error.message, "not found");

    Ok(())
}
