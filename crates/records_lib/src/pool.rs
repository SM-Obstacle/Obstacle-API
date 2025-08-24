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

#[inline(always)]
#[cfg(feature = "test")]
const fn empty_query_results() -> std::iter::Empty<std::iter::Empty<sea_orm::MockRow>> {
    std::iter::empty::<std::iter::Empty<sea_orm::MockRow>>()
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

    /// Returns the database from the URL of the Redis database, and the backend of the SQL database,
    /// with initial data for the mock database.
    ///
    /// This is used for testing, by simulating an SQL database.
    #[cfg(feature = "test")]
    #[cfg_attr(nightly, doc(cfg(feature = "test")))]
    pub fn from_mock_db_with_initial<I1, I2>(
        db_backend: sea_orm::DbBackend,
        redis_url: String,
        query_results: I1,
        exec_results: I2,
    ) -> Result<Self, deadpool_redis::CreatePoolError>
    where
        I1: IntoIterator<Item: IntoIterator<Item: sea_orm::IntoMockRow>>,
        I2: IntoIterator<Item = sea_orm::MockExecResult>,
    {
        let db_conn = sea_orm::MockDatabase::new(db_backend)
            .append_query_results(query_results)
            .append_exec_results(exec_results)
            .into_connection();
        Self::from_db_conn(db_conn, redis_url)
    }

    /// Returns the database from the URL of the Redis database, and the backend of the SQL database,
    /// with initial query results for the mock database.
    ///
    /// This is used for testing, by simulating an SQL database.
    #[cfg(feature = "test")]
    #[cfg_attr(nightly, doc(cfg(feature = "test")))]
    pub fn from_mock_db_with_query_results<I>(
        db_backend: sea_orm::DbBackend,
        redis_url: String,
        query_results: I,
    ) -> Result<Self, deadpool_redis::CreatePoolError>
    where
        I: IntoIterator<Item: IntoIterator<Item: sea_orm::IntoMockRow>>,
    {
        Self::from_mock_db_with_initial(db_backend, redis_url, query_results, [])
    }

    /// Returns the database from the URL of the Redis database, and the backend of the SQL database,
    /// with initial exec results for the mock database.
    ///
    /// This is used for testing, by simulating an SQL database.
    #[cfg(feature = "test")]
    #[cfg_attr(nightly, doc(cfg(feature = "test")))]
    pub fn from_mock_db_with_exec_results<I>(
        db_backend: sea_orm::DbBackend,
        redis_url: String,
        exec_results: I,
    ) -> Result<Self, deadpool_redis::CreatePoolError>
    where
        I: IntoIterator<Item = sea_orm::MockExecResult>,
    {
        Self::from_mock_db_with_initial(db_backend, redis_url, empty_query_results(), exec_results)
    }

    /// Returns the database from the URL of the Redis database, and the backend of the SQL database,
    /// with no data in the mock database.
    ///
    /// This is used for testing, by simulating an SQL database.
    #[cfg(feature = "test")]
    #[cfg_attr(nightly, doc(cfg(feature = "test")))]
    pub fn from_mock_db(
        db_backend: sea_orm::DbBackend,
        redis_url: String,
    ) -> Result<Self, deadpool_redis::CreatePoolError> {
        Self::from_mock_db_with_initial(db_backend, redis_url, empty_query_results(), [])
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
