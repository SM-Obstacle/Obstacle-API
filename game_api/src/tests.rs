use crate::init_env;
use anyhow::Context;
use records_lib::{get_mysql_pool, get_redis_pool, Database};

pub async fn init_db() -> anyhow::Result<Database> {
    match dotenvy::dotenv() {
        Ok(_) => (),
        Err(err) if err.not_found() => (),
        Err(other) => return Err(other).context("retrieving .env files"),
    }

    let env = init_env()?;

    Ok(Database {
        mysql_pool: get_mysql_pool(env.db_env.db_url.db_url).await?,
        redis_pool: get_redis_pool(env.db_env.redis_url.redis_url)?,
    })
}

#[macro_export]
macro_rules! init_app {
    ($(.$func:ident($($t:tt)*))*) => {{
        use actix_web::test;

        let db = $crate::tests::init_db().await?;

        (
            test::init_service(
                actix_web::App::new()
                    .wrap(tracing_actix_web::TracingLogger::default())
                    .app_data(db.clone())
                    $(.$func($($t)*))*
            )
            .await,
            db
        )
    }};
}
