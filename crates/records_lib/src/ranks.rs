//! Module which contains utility functions used to update maps leaderboards and get players ranks.

use std::collections::{HashMap, HashSet};

use crate::{RedisConnection, error::RecordsResult, opt_event::OptEvent, redis_key::map_key};
use deadpool_redis::redis;
use entity::{event_edition_records, records};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter as _, QuerySelect, QueryTrait as _,
    Select,
    sea_query::{Func, expr},
};

/// Selects the records of the provided maps, restricted to the optional event.
fn find_records_of<I>(map_ids: I, event: OptEvent<'_>) -> Select<records::Entity>
where
    I: IntoIterator<Item = u32>,
{
    records::Entity::find()
        .filter(records::Column::MapId.is_in(map_ids))
        .apply_if(event.get(), |query, (ev, ed)| {
            query.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
}

/// Counts the records of each of the provided maps, in the database.
///
/// Maps without any record are absent from the returned map.
async fn count_records_maps<C, I>(
    conn: &C,
    map_ids: I,
    event: OptEvent<'_>,
) -> RecordsResult<HashMap<u32, u64>>
where
    C: ConnectionTrait,
    I: IntoIterator<Item = u32>,
{
    let counts = find_records_of(map_ids, event)
        .select_only()
        .column(records::Column::MapId)
        .expr(Func::count_distinct(expr::Expr::col(
            records::Column::RecordPlayerId,
        )))
        .group_by(records::Column::MapId)
        // MariaDB answers `COUNT` with a signed `BIGINT`, whatever the counted column.
        .into_tuple::<(u32, i64)>()
        .all(conn)
        .await?;

    Ok(counts
        .into_iter()
        .map(|(map_id, count)| (map_id, count as u64))
        .collect())
}

/// Rebuilds the Redis leaderboards of the provided maps from the database, unconditionally.
async fn force_update_leaderboards<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_ids: &[u32],
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    let rows = find_records_of(map_ids.iter().copied(), event)
        .select_only()
        .column(records::Column::MapId)
        .column(records::Column::RecordPlayerId)
        .column_as(expr::Expr::col(records::Column::Time).min(), "time")
        .group_by(records::Column::MapId)
        .group_by(records::Column::RecordPlayerId)
        .into_tuple::<(u32, u32, i32)>()
        .all(conn)
        .await?;

    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    // Drop the stale leaderboards first, so that maps whose records all disappeared don't keep
    // their entries. A sorted set orders itself by score, so the rows need no ordering.
    for &map_id in map_ids {
        pipe.del(map_key(map_id, event));
    }
    for (map_id, player_id, time) in rows {
        pipe.zadd(map_key(map_id, event), player_id, time);
    }

    let _: () = pipe.query_async(redis_conn).await?;

    Ok(())
}

/// Checks whether the Redis leaderboards of the provided maps hold as many records as the
/// database does, and regenerates the ones that don't.
///
/// This is both a repair of any drift between MariaDB and Redis, and the guarantee that a map
/// has a leaderboard at all: Redis answers `ZCOUNT` with `0` on a missing key, which is
/// indistinguishable from a player being first, so ranks read off an unpopulated map would all
/// come back as 1.
///
/// Whatever the number of maps, this costs one database query and one Redis round trip, plus
/// one of each when some leaderboard actually needs rebuilding.
///
/// It returns the number of records of each map, in the database.
pub async fn update_leaderboards<C, I>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_ids: I,
    event: OptEvent<'_>,
) -> RecordsResult<HashMap<u32, u64>>
where
    C: ConnectionTrait,
    I: IntoIterator<Item = u32>,
{
    let map_ids: Vec<u32> = map_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if map_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mysql_counts = count_records_maps(conn, map_ids.iter().copied(), event).await?;

    let mut pipe = redis::pipe();
    for &map_id in &map_ids {
        pipe.zcard(map_key(map_id, event));
    }
    let redis_counts: Vec<u64> = pipe.query_async(redis_conn).await?;

    let stale: Vec<u32> = map_ids
        .iter()
        .zip(redis_counts)
        .filter(|(map_id, redis_count)| {
            mysql_counts.get(*map_id).copied().unwrap_or(0) != *redis_count
        })
        .map(|(map_id, _)| *map_id)
        .collect();

    if !stale.is_empty() {
        force_update_leaderboards(conn, redis_conn, &stale, event).await?;
    }

    Ok(mysql_counts)
}

/// Checks if the Redis leaderboard for the map with the provided ID has a different count
/// than in the database, and regenerates the Redis leaderboard completely if so.
///
/// This is a check to avoid differences between the MariaDB and the Redis leaderboards.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
) -> RecordsResult<u64> {
    let counts = update_leaderboards(conn, redis_conn, [map_id], event).await?;

    Ok(counts.get(&map_id).copied().unwrap_or(0))
}

/// Gets the ranks of many times at once, in the order they were provided.
///
/// Every lookup is sent in a single pipelined round trip, instead of one round trip per record.
/// Prefer it whenever the ranks of a whole set of records are needed, like when filling a
/// leaderboard or a page of records, as a sequential version makes the latency grow with the
/// page size.
///
/// Each item is a `(map_id, time)` pair. They may refer to different maps, but they all belong
/// to the same optional `event`. The leaderboards of every map involved are brought in sync
/// with the database first, with [`update_leaderboards`].
///
/// ## Example
///
/// ```ignore
/// let ranks = ranks::get_ranks(
///     conn,
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
pub async fn get_ranks<C, I>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    times: I,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<i32>>
where
    C: ConnectionTrait,
    I: IntoIterator<Item = (u32, i32)>,
{
    let times: Vec<(u32, i32)> = times.into_iter().collect();

    // Querying an empty pipeline still costs a round trip, and Redis has nothing to answer.
    if times.is_empty() {
        return Ok(Vec::new());
    }

    update_leaderboards(
        conn,
        redis_conn,
        times.iter().map(|(map_id, _)| *map_id),
        event,
    )
    .await?;

    let mut pipe = redis::pipe();
    for &(map_id, time) in &times {
        pipe.zcount(map_key(map_id, event), "-inf", time - 1);
    }

    let counts: Vec<i32> = pipe.query_async(redis_conn).await?;

    Ok(counts.into_iter().map(|count| count + 1).collect())
}

/// Gets the rank of the time of a player on a map.
///
/// This is the single-record counterpart of [`get_ranks`], and shares its guarantee that the
/// map's leaderboard is in sync with the database before being read.
///
/// ## Example
///
/// ```ignore
/// let rank1 = ranks::get_rank(conn, &mut redis_conn, map_id, time1, Default::default()).await?;
/// let rank2 = ranks::get_rank(conn, &mut redis_conn, map_id, time2, Default::default()).await?;
/// ```
pub async fn get_rank<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    time: i32,
    event: OptEvent<'_>,
) -> RecordsResult<i32> {
    let ranks = get_ranks(conn, redis_conn, [(map_id, time)], event).await?;

    // `get_ranks` yields exactly one rank per provided time.
    Ok(ranks.into_iter().next().unwrap_or(1))
}
