mod base;

use actix_http::StatusCode;
use actix_web::test;
use records_lib::{Database, pool::get_redis_pool};
use sea_orm::DbBackend;

#[tokio::test]
async fn test_not_found() -> anyhow::Result<()> {
    let env = base::get_env()?;
    let db = Database::from_mock_db(DbBackend::MySql, env.db_env.redis_url.redis_url)?;
    let app = base::get_app(db).await;
    let req = test::TestRequest::get().uri("/").to_request();

    let resp = test::call_service(&app, req).await;
    let status_code = resp.status();

    let body = test::read_body(resp).await;
    let error: base::ErrorResponse = serde_json::from_slice(&body)?;

    assert_eq!(status_code, StatusCode::NOT_FOUND);
    assert_eq!(error.r#type, 301);
    assert_eq!(error.message, "not found");

    Ok(())
}

#[tokio::test]
async fn test_info() -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    struct ApiStatus<'a> {
        kind: &'a str,
        #[allow(dead_code)]
        at: chrono::NaiveDateTime,
    }

    #[derive(serde::Deserialize)]
    struct InfoResponse<'a> {
        #[allow(dead_code)]
        service_name: &'a str,
        #[allow(dead_code)]
        contacts: &'a str,
        #[allow(dead_code)]
        api_version: &'a str,
        status: ApiStatus<'a>,
    }

    let env = base::get_env()?;

    base::wrap(env.db_env.db_url.db_url, async |sql_conn| {
        let db = Database {
            sql_conn,
            redis_pool: get_redis_pool(env.db_env.redis_url.redis_url)?,
        };

        let app = base::get_app(db).await;
        let req = test::TestRequest::get().uri("/info").to_request();

        let resp = test::call_service(&app, req).await;
        let status_code = resp.status();

        let body = test::read_body(resp).await;
        let body: InfoResponse = base::try_from_slice(&body)?;

        assert_eq!(status_code, StatusCode::OK);
        assert_eq!(body.status.kind, "Normal");

        anyhow::Ok(())
    })
    .await?;

    Ok(())
}
