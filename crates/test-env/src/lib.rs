use std::{env, panic};

use anyhow::Context as _;
use futures::FutureExt as _;
use migration::MigratorTrait as _;
use mkenv::prelude::*;
use records_lib::{Database, DbEnv, pool::get_redis_pool};
use sea_orm::{ConnectionTrait as _, DbConn};
use tracing_subscriber::fmt::TestWriter;

fn is_db_drop_forced() -> bool {
    env::args_os().any(|arg| arg == "--force-drop-db")
}

pub fn get_map_id() -> u32 {
    rand::random_range(..u32::MAX)
}

pub trait IntoResult {
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

pub fn init_env() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Err(err) if !err.not_found() => return Err(err).context("cannot retrieve .env files"),
        _ => (),
    }

    let _ = tracing_subscriber::fmt()
        .with_writer(TestWriter::new())
        .try_init();

    Ok(())
}

pub async fn wrap<F, R>(test: F) -> anyhow::Result<<R as IntoResult>::Out>
where
    F: AsyncFnOnce(Database) -> R,
    R: IntoResult,
{
    init_env()?;
    let env = DbEnv::define();

    let master_db = sea_orm::Database::connect(&env.db_url.db_url.get()).await?;

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
    tracing::info!("Created database {db_name}");

    let db = match master_db {
        #[cfg(feature = "mysql")]
        sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(_) => {
            use sea_orm::sqlx;

            let connect_options = master_db.get_mysql_connection_pool().connect_options();
            let connect_options = (*connect_options).clone();
            let options = connect_options.database(&db_name);
            let db = sqlx::mysql::MySqlPool::connect_with(options).await?;
            DbConn::from(db)
        }
        #[cfg(feature = "postgres")]
        sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(_) => {
            use sea_orm::sqlx;

            let connect_options = master_db.get_postgres_connection_pool().connect_options();
            let connect_options = (*connect_options).clone();
            let options = connect_options.database(&db_name);
            let db = sqlx::postgres::PgPool::connect_with(options).await?;
            DbConn::from(db)
        }
        _ => unreachable!("must enable either `mysql` or `postgres` feature for testing"),
    };

    migration::Migrator::up(&db, None).await?;

    let r = panic::AssertUnwindSafe(test(Database {
        sql_conn: db,
        redis_pool: get_redis_pool(env.redis_url.redis_url.get())?,
    }))
    .catch_unwind()
    .await;

    if is_db_drop_forced() {
        master_db
            .execute_unprepared(&format!("drop database {db_name}"))
            .await?;
        tracing::info!("Database {db_name} force-deleted");
        match r {
            Ok(r) => r.into_result(),
            Err(e) => {
                tracing::info!("Test failed");
                panic::resume_unwind(e)
            }
        }
    } else {
        match r.map(IntoResult::into_result) {
            Ok(Ok(out)) => {
                master_db
                    .execute_unprepared(&format!("drop database {db_name}"))
                    .await?;
                Ok(out)
            }
            other => {
                tracing::info!(
                    "Test failed, leaving database {db_name} as-is. \
                    Run with `--force-drop-db` to drop the database everytime."
                );
                match other {
                    Ok(Err(e)) => Err(e),
                    Err(e) => panic::resume_unwind(e),
                    _ => unreachable!(),
                }
            }
        }
    }
}
