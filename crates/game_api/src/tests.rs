mod root;

use std::fmt;

use actix_http::Request;
use actix_web::{
    App, Error,
    body::MessageBody,
    dev::{Service, ServiceResponse},
    test,
};
use anyhow::Context;
use migration::MigratorTrait as _;
use records_lib::Database;
use sea_orm::{ConnectionTrait, DbConn};
use tracing_actix_web::TracingLogger;

use crate::{configure, init_env};

#[derive(Debug, serde::Deserialize)]
struct ErrorResponse<'a> {
    #[allow(dead_code)]
    request_id: &'a str,
    r#type: i32,
    message: &'a str,
}

fn get_env() -> anyhow::Result<crate::InitEnvOut> {
    match dotenvy::dotenv() {
        Err(err) if !err.not_found() => return Err(err).context("retrieving .env files"),
        _ => (),
    }

    init_env()
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

#[derive(Debug)]
pub enum ApiError {
    InvalidJson,
    Error { r#type: i32, message: String },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::InvalidJson => f.write_str("Invalid JSON returned by the API"),
            ApiError::Error { r#type, message } => {
                f.write_str("Error returned from API: ")?;
                f.debug_map()
                    .entry(&"type", r#type)
                    .entry(&"message", message)
                    .finish()
            }
        }
    }
}

impl std::error::Error for ApiError {}

trait IntoResult {
    type Out;

    fn into_result(self) -> anyhow::Result<Self::Out>;
}

impl IntoResult for () {
    type Out = ();

    fn into_result(self) -> anyhow::Result<Self::Out> {
        Ok(())
    }
}

impl<T, E> IntoResult for Result<T, E>
where
    anyhow::Error: From<E>,
{
    type Out = T;

    fn into_result(self) -> anyhow::Result<Self::Out> {
        self.map_err(From::from)
    }
}

async fn wrap<F, R>(db_url: String, test: F) -> anyhow::Result<<R as IntoResult>::Out>
where
    F: AsyncFnOnce(DbConn) -> R,
    R: IntoResult,
{
    let master_db = sea_orm::Database::connect(&db_url).await?;

    // For some reasons, on MySQL/MariaDB, using a schema name with some capital letters
    // may produce the error code 1932 (42S02) "Table 'X' doesn't exist in engine" when
    // doing a query.
    let db_name = format!(
        "_test_db_{}",
        records_lib::gen_random_str(10).to_lowercase()
    );

    master_db
        .execute_unprepared(&format!("create database {db_name}"))
        .await?;

    let db = match master_db {
        #[cfg(feature = "mysql")]
        sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(_) => {
            let connect_options = master_db.get_mysql_connection_pool().connect_options();
            let connect_options = (*connect_options).clone();
            let options = connect_options.database(&db_name);
            let db = sqlx::mysql::MySqlPool::connect_with(options).await?;
            DbConn::from(db)
        }
        #[cfg(feature = "postgres")]
        sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(_) => {
            let connect_options = master_db.get_postgres_connection_pool().connect_options();
            let connect_options = (*connect_options).clone();
            let options = connect_options.database(&db_name);
            let db = sqlx::postgres::PgPool::connect_with(options).await?;
            DbConn::from(db)
        }
        _ => unreachable!(),
    };

    migration::Migrator::up(&db, None).await?;

    let r = test(db).await;
    match r.into_result() {
        Ok(out) => {
            master_db
                .execute_unprepared(&format!("drop database {db_name}"))
                .await?;
            Ok(out)
        }
        Err(e) => Err(e),
    }
}

fn try_from_slice<'de, T>(slice: &'de [u8]) -> Result<T, ApiError>
where
    T: serde::Deserialize<'de>,
{
    match serde_json::from_slice(slice) {
        Ok(t) => Ok(t),
        Err(_) => match serde_json::from_slice::<ErrorResponse>(slice) {
            Ok(err) => Err(ApiError::Error {
                r#type: err.r#type,
                message: err.message.to_owned(),
            }),
            Err(_) => Err(ApiError::InvalidJson),
        },
    }
}
