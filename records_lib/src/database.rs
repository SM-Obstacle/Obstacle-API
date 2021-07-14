pub use deadpool_redis::Pool as RedisPool;
pub use sqlx::MySqlPool;

#[derive(Clone)]
pub struct Database {
    pub mysql_pool: MySqlPool,
    pub redis_pool: RedisPool,
}
