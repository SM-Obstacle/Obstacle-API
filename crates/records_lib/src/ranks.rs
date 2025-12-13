//! Module which contains utility functions used to update maps leaderboards and get players ranks.

use crate::{
    RedisConnection, RedisPool, error::RecordsResult, opt_event::OptEvent, redis_key::map_key,
};
use deadpool_redis::{
    PoolError,
    redis::{self, AsyncCommands},
};
use entity::{event_edition_records, records};
use futures::TryStreamExt;
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, Order, PaginatorTrait, QueryFilter as _,
    QueryOrder as _, QuerySelect, QueryTrait as _, SelectModel, Selector, StreamTrait,
    sea_query::expr,
};

async fn count_records_map<C: ConnectionTrait>(
    conn: &C,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let query = records::Entity::find()
        .filter(records::Column::MapId.eq(map_id))
        .group_by(records::Column::RecordPlayerId);

    let query = match event.get() {
        Some((ev, ed)) => query.reverse_join(event_edition_records::Entity).filter(
            event_edition_records::Column::EventId
                .eq(ev.id)
                .and(event_edition_records::Column::EditionId.eq(ed.id)),
        ),
        None => query,
    };

    let result = query.count(conn).await?;
    Ok(result)
}

/// Checks if the Redis leaderboard for the map with the provided ID has a different count
/// than in the database, and regenerates the Redis leaderboard completely if so.
///
/// This is a check to avoid differences between the MariaDB and the Redis leaderboards.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let mysql_count = count_records_map(conn, map_id, event).await?;

    let key = map_key(map_id, event);

    let mut redis_conn = redis_pool.get().await?;
    let redis_count: u64 = redis_conn.zcount(key, "-inf", "+inf").await?;

    if redis_count != mysql_count {
        force_update_locked(conn, redis_pool, map_id, event).await?;
    }

    Ok(mysql_count)
}

/// A leaderboard row.
#[derive(Debug, sea_orm::FromQueryResult)]
pub struct DbLeaderboardItem {
    /// The ID of the player who made the record.
    pub record_player_id: u32,
    /// The record time.
    pub time: i32,
}

fn get_mariadb_lb_query(
    map_id: u32,
    event: OptEvent<'_>,
) -> Selector<SelectModel<DbLeaderboardItem>> {
    records::Entity::find()
        .filter(records::Column::MapId.eq(map_id))
        .group_by(records::Column::RecordPlayerId)
        .order_by(records::Column::Time.min(), Order::Asc)
        .order_by(records::Column::RecordPlayerId, Order::Asc)
        .apply_if(event.get(), |builder, (ev, ed)| {
            builder.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
        .select_only()
        .column(records::Column::RecordPlayerId)
        .column_as(expr::Expr::col(records::Column::Time).min(), "time")
        .into_model()
}

async fn force_update_locked<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    let mut redis_conn = redis_pool.get().await?;

    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    let key = map_key(map_id, event).to_string();

    pipe.del(&key);

    get_mariadb_lb_query(map_id, event)
        .stream(conn)
        .await?
        .map_ok(|item| {
            pipe.zadd(&key, item.record_player_id, item.time);
        })
        .try_collect::<()>()
        .await?;

    let _: () = pipe.query_async(&mut redis_conn).await?;

    Ok(())
}

// This is just a Redis connection retrieved from the pool, we just don't expose it to make sure
// we exlusively own it in the body of our rank retrieval implementation.
/// A current ranking session.
///
/// See the documentation of the [`get_rank_in_session`] function for more information.
pub struct RankingSession {
    redis_conn: RedisConnection,
}

impl RankingSession {
    /// Returns a new ranking session from the provided Redis pool.
    pub async fn try_from_pool(pool: &RedisPool) -> Result<Self, PoolError> {
        let redis_conn = pool.get().await?;
        Ok(Self { redis_conn })
    }
}

/// Gets the rank of the time of a player on a map, using the current ranking session.
///
/// This is like [`get_rank`], but used when retrieving a large amount of ranks during an operation,
/// to avoid creating a new Redis connection from the pool each time.
///
/// ## Example
///
/// ```ignore
/// let mut session = ranks::RankingSession::try_from_pool(&pool).await?;
/// let rank1 =
///   ranks::get_rank_in_session(&mut session, map_id, player1_id, time1, Default::default())
///     .await?;
/// let rank2 =
///   ranks::get_rank_in_session(&mut session, map_id, player2_id, time2, Default::default())
///     .await?;
/// ```
///
/// See the documentation of the [`get_rank`] function for more information.
pub async fn get_rank_in_session(
    session: &mut RankingSession,
    map_id: u32,
    player_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let key = map_key(map_id, event);

    // Loop until the commands response isn't null, meaning the watchpoint didn't trigger during the
    // transaction, meaning it succeeded.
    loop {
        redis::cmd("WATCH")
            .arg(&key)
            .exec_async(&mut session.redis_conn)
            .await?;

        let score: Option<i32> = session.redis_conn.zscore(&key, player_id).await?;

        let mut pipe = redis::pipe();
        pipe.atomic()
            .zadd(&key, player_id, time)
            .ignore()
            .zcount(&key, "-inf", time - 1);

        // Restore the previous state
        let _ = match score {
            Some(old_time) => pipe.zadd(&key, player_id, old_time).ignore(),
            None => pipe.zrem(&key, player_id).ignore(),
        };

        let response: Option<(i32,)> = pipe.query_async(&mut session.redis_conn).await?;

        if let Some((count,)) = response {
            return Ok(count + 1);
        }
    }
}

/// Gets the rank of the time of a player on a map.
///
/// Note: if you intend to use this function in a loop, prefer using the [`get_rank_in_session`]
/// function instead.
///
/// This function is concurrency-safe, so it guarantees that at the moment it is called, it returns
/// the correct rank for the provided time. It also keeps the leaderboard unchanged.
///
/// However, it doesn't guarantee that the related ZSET of the map's leaderboard is synchronized
/// with its SQL database version. For this, please use the [`update_leaderboard`] function.
///
/// The ranking type is the standard competition ranking (1224).
pub async fn get_rank(
    redis_pool: &RedisPool,
    map_id: u32,
    player_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let mut session = RankingSession::try_from_pool(redis_pool).await?;
    get_rank_in_session(&mut session, map_id, player_id, time, event).await
}
