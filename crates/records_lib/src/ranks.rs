//! Module which contains utility functions used to update maps leaderboards and get players ranks.

use crate::{
    RedisConnection, RedisPool, error::RecordsResult, opt_event::OptEvent, redis_key::map_key,
};
use deadpool_redis::redis::{self, AsyncCommands};
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

/// Gets the rank of the time of a player on a map.
///
/// ## Example
///
/// ```ignore
/// let rank1 =
///   ranks::get_rank_in_session(&mut redis_conn, map_id, time1, Default::default())
///     .await?;
/// let rank2 =
///   ranks::get_rank_in_session(&mut redis_conn, map_id, time2, Default::default())
///     .await?;
/// ```
pub async fn get_rank(
    redis_conn: &mut RedisConnection,
    map_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let key = map_key(map_id, event);
    let count: i32 = redis_conn.zcount(key, "-inf", time - 1).await?;
    Ok(count + 1)
}

/// Gets the ranks of many times at once, in the order they were provided.
///
/// This is the batched counterpart of [`get_rank`]: every lookup is sent in a single
/// pipelined round trip, instead of one round trip per record. Prefer it whenever the
/// ranks of a whole set of records are needed, like when filling a leaderboard or a page
/// of records, as the sequential version makes the latency grow with the page size.
///
/// Each item is a `(map_id, time)` pair. They may refer to different maps, but they all
/// belong to the same optional `event`.
///
/// ## Example
///
/// ```ignore
/// let ranks = ranks::get_ranks(
///     &mut redis_conn,
///     records.iter().map(|record| (record.map_id, record.time)),
///     Default::default(),
/// )
/// .await?;
///
/// for (record, rank) in records.into_iter().zip(ranks) {
///     // ...
/// }
/// ```
pub async fn get_ranks<I>(
    redis_conn: &mut RedisConnection,
    times: I,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<i32>>
where
    I: IntoIterator<Item = (u32, i32)>,
{
    let mut pipe = redis::pipe();
    let mut is_empty = true;

    for (map_id, time) in times {
        pipe.zcount(map_key(map_id, event), "-inf", time - 1);
        is_empty = false;
    }

    // Querying an empty pipeline still costs a round trip, and Redis has nothing to answer.
    if is_empty {
        return Ok(Vec::new());
    }

    let counts: Vec<i32> = pipe.query_async(redis_conn).await?;

    Ok(counts.into_iter().map(|count| count + 1).collect())
}
