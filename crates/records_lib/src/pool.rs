//! Contains types to represent database pools.

use deadpool_redis::Runtime;
use sea_orm::DbConn;

use crate::RedisPool;

/// Represents the database of the API, meaning an SQL database, and a Redis database.
pub struct Database {
    /// The SQL database connection pool. This can also be a testing database [`DbConn::MockDatabaseConnection`].
    pub sql_conn: DbConn,
    /// The Redis pool.
    pub redis_pool: RedisPool,
}

/// The error raised by the creation of a [`Database`].
#[derive(Debug, thiserror::Error)]
pub enum DatabaseCreationError {
    /// Error raised when connecting to the SQL database.
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    /// Error raised when creating the Redis pool.
    #[error(transparent)]
    DeadpoolRedis(#[from] deadpool_redis::CreatePoolError),
}

impl Database {
    fn from_db_conn(
        db_conn: DbConn,
        redis_url: String,
    ) -> Result<Self, deadpool_redis::CreatePoolError> {
        let redis_pool = get_redis_pool(redis_url)?;
        Ok(Self {
            sql_conn: db_conn,
            redis_pool,
        })
    }

    /// Returns the database from the URL to the SQL and Redis databases.
    pub async fn from_db_url(
        db_url: String,
        redis_url: String,
    ) -> Result<Self, DatabaseCreationError> {
        let db_conn = sea_orm::Database::connect(db_url).await?;
        Self::from_db_conn(db_conn, redis_url).map_err(From::from)
    }

    /// Returns the database from the URL of the Redis database, and the backend of the SQL database.
    ///
    /// This is used for testing, by simulating an SQL database.
    #[cfg(feature = "test")]
    #[cfg_attr(nightly, doc(cfg(feature = "test")))]
    pub fn from_mock_db(
        db_backend: sea_orm::DbBackend,
        redis_url: String,
    ) -> Result<Self, deadpool_redis::CreatePoolError> {
        let db_conn = sea_orm::MockDatabase::new(db_backend).into_connection();
        Self::from_db_conn(db_conn, redis_url)
    }
}

// For some reasons, sea_orm::DbConn doesn't implement Clone
impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            redis_pool: self.redis_pool.clone(),
            sql_conn: match &self.sql_conn {
                #[cfg(feature = "mysql")]
                sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(conn) => {
                    sea_orm::DatabaseConnection::SqlxMySqlPoolConnection(conn.clone())
                }
                #[cfg(feature = "test")]
                sea_orm::DatabaseConnection::MockDatabaseConnection(conn) => {
                    sea_orm::DatabaseConnection::MockDatabaseConnection(conn.clone())
                }
                #[cfg(feature = "postgres")]
                sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(conn) => {
                    sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(conn.clone())
                }
                #[cfg(feature = "sqlite")]
                sea_orm::DatabaseConnection::SqlxSqlitePoolConnection(conn) => {
                    sea_orm::DatabaseConnection::SqlxSqlitePoolConnection(conn.clone())
                }
                #[cfg(feature = "sea-orm-proxy")]
                sea_orm::DatabaseConnection::ProxyDatabaseConnection(conn) => {
                    sea_orm::DatabaseConnection::ProxyDatabaseConnection(conn.clone())
                }
                sea_orm::DatabaseConnection::Disconnected => {
                    sea_orm::DatabaseConnection::Disconnected
                }
            },
        }
    }
}

/// Creates and returns the Redis pool with the provided URL.
pub fn get_redis_pool(url: String) -> Result<RedisPool, deadpool_redis::CreatePoolError> {
    let cfg = deadpool_redis::Config {
        url: Some(url),
        connection: None,
        pool: None,
    };
    cfg.create_pool(Some(Runtime::Tokio1))
}
