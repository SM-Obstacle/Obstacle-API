//! Module which contains utility functions used to update maps leaderboards and get players ranks.

use crate::{RedisConnection, error::RecordsResult, opt_event::OptEvent, redis_key::map_key};
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
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let mysql_count = count_records_map(conn, map_id, event).await?;

    let key = map_key(map_id, event);

    let redis_count: u64 = redis_conn.zcount(key, "-inf", "+inf").await?;
    if redis_count != mysql_count {
        force_update_locked(conn, redis_conn, map_id, event).await?;
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
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
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

    let _: () = pipe.query_async(redis_conn).await?;

    Ok(())
}

/// Gets the rank of a time in a map.
///
/// The ranking type is the standard competition ranking (1224).
pub async fn get_rank(
    redis_conn: &mut RedisConnection,
    map_id: u32,
    player_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let key = map_key(map_id, event);

    // We update the Redis leaderboard if it doesn't have the requested `time`, and keep
    // track of the previous time if it's lower than ours.
    let score: Option<i32> = redis_conn.zscore(&key, player_id).await?;
    let newest_time = score.filter(|t| *t < time);

    let mut pipe = redis::pipe();
    pipe.atomic()
        .zadd(&key, player_id, time)
        .ignore()
        .zcount(&key, "-inf", time - 1);
    if let Some(old_time) = newest_time {
        pipe.zadd(&key, player_id, old_time).ignore();
    }

    let response: [i32; 1] = pipe.query_async(redis_conn).await?;
    Ok(response[0] + 1)
}
