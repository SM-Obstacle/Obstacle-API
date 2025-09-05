mod base;

use actix_http::StatusCode;
use actix_web::test;
use game_api_lib::TracedError;
use records_lib::Database;
use sea_orm::DbBackend;

#[tokio::test]
async fn test_not_found() -> anyhow::Result<()> {
    let env = base::get_env()?;
    let db = Database::from_mock_db(DbBackend::MySql, env.db_env.redis_url.redis_url)?;
    let app = base::get_app(db).await;
    let req = test::TestRequest::get().uri("/").to_request();

    let resp = test::try_call_service(&app, req).await;
    let err = resp.err().expect("Response should be error");
    let err = err
        .as_error::<TracedError>()
        .expect("Response should be a traced error");
    assert_eq!(err.status_code, Some(StatusCode::NOT_FOUND));
    // Not found error type
    assert_eq!(err.r#type, Some(301));

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

    base::with_db(async |db| {
        let app = base::get_app(db).await;
        let req = test::TestRequest::get().uri("/info").to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status();

        let body = test::read_body(resp).await;
        let body = base::try_from_slice::<InfoResponse>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.status.kind, "Normal");

        anyhow::Ok(())
    })
    .await?;

    Ok(())
}
