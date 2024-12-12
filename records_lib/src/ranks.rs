//! This is a tiny module which contains utility functions used to update the maps leaderboards
//! in Redis.
//!
//! ## Leaderboards state
//!
//! You should always remember that the state of the MariaDB and the Redis leaderboards are not
//! always the same. The problem is that we can't flag it because they could be different
//! for different reasons:
//!
//! - Records migration
//! - Redis that failed to update the score of a player on a map
//! - The associated Redis key has been deleted
//! - etc.
//!
//! We mainly use the Redis leaderboard to get the rank of a player on a map. Thus, we should take
//! care in each operation and update it if anything goes wrong.
//!
//! Let's say we have a map with this leaderboard in MariaDB:
//!
//! | Rank | Player | Time |
//! | ----- | ----- | ----- |
//! | 1 | Player1 | 01:50.33 |
//! | 2 | Player2 | 01:56.42 |
//! | 2 | Player3 | 01:56.42 |
//! | 4 | Player4 | 02:09.25 |
//!
//! As you can see, we're using the competitive ranking (1224). However, the Redis ranking system
//! of ZSETs isn't using this ranking system. The technique is to get the score of the player
//! we want, then get a list of the players with the same score, to get the rank of the first player.
//!
//! For instance, to get the rank of Player3, we get his score: 01:56.42. Then, we get all
//! the players with the same score, which returns the list: `[Player2, Player3]` . Then, we get
//! the rank of the first player, Player2, which is 2.
//!
//! This technique works well if the two leaderboards, from MariaDB and Redis,
//! are **in the same state**. But sometimes, it's not the case. Let's say the Redis leaderboard is:
//!
//! | Rank | Player | Time |
//! | ----- | ----- | ----- |
//! | 1 | Player1 | 01:50.33 |
//! | 2 | Player2 | 01:56.42 |
//! | 3 | Player4 | 02:09.25 |
//! | 4 | Player3 | 03:13.87 |
//!
//! As you can see, the true rank of Player3 is still 2nd, but with Redis, he would be 4th.
//! If, at the moment, we already know the real time of Player3, which is 01:56.42, we can make
//! a mechanism that checks the time of the player, and updates the Redis leaderboard if they're different.
//! But if we don't already know his time, then we can't tell if the returned rank from Redis
//! is valid or not.
//!
//! This is why the [`get_rank`] function requires the time of the player.

use crate::{
    context::{HasMapId, HasPlayerId},
    error::{RecordsError, RecordsResult},
    redis_key::{map_key, MapKey},
    DatabaseConnection, RedisConnection,
};
use deadpool_redis::redis::{self, AsyncCommands};
use futures::TryStreamExt;
use itertools::{EitherOrBoth, Itertools};

/// Updates the rank of a player on a map.
///
/// This is roughly just a `ZADD` command for the Redis leaderboard of the map.
/// The difference is that it locks the leaderboard during the operation so that any other request
/// that might access the ranks of the leaderboard must wait for it to finish.
///
/// # Arguments
///
/// * `redis_conn`: a connection to the Redis server
/// * `map_id`: The ID of the map
/// * `player_id`: The ID of the player
/// * `time`: The time of the player on this map
/// * `event`: The event context if provided
pub async fn update_rank<C>(conn: &mut RedisConnection, ctx: C, time: i32) -> RecordsResult<()>
where
    C: HasMapId + HasPlayerId,
{
    let (): _ = conn
        .zadd(
            map_key(ctx.get_map_id(), ctx.get_opt_event_edition()),
            ctx.get_player_id(),
            time,
        )
        .await?;

    Ok(())
}

async fn count_records_map<C: HasMapId>(
    conn: &mut sqlx::MySqlConnection,
    ctx: C,
) -> RecordsResult<i64> {
    let builder = ctx.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM (SELECT r.* FROM records r ");
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" ");
    let query = builder
        .push_event_filter(&mut query, "eer")
        .push(" group by record_player_id) r")
        .build_query_scalar();

    query.fetch_one(conn).await.map_err(Into::into)
}

/// Checks if the Redis leaderboard for the map with the provided ID has a different count
/// than in the database, and reupdates the Redis leaderboard completly if so.
///
/// This is a check to avoid differences between the MariaDB and the Redis leaderboards.
///
/// It returns the number of records in the map.
pub async fn update_leaderboard<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> RecordsResult<i64> {
    let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition());

    let redis_count: i64 = conn.redis_conn.zcount(key, "-inf", "+inf").await?;
    let mysql_count: i64 = count_records_map(conn.mysql_conn, &ctx).await?;

    if redis_count != mysql_count {
        force_update(conn, ctx).await?;
    }

    Ok(mysql_count)
}

fn get_mariadb_lb_query<C: HasMapId>(ctx: C) -> sqlx::QueryBuilder<'static, sqlx::MySql> {
    let builder = ctx.sql_frag_builder();

    let mut q = sqlx::QueryBuilder::new(
        "select record_player_id, min(time) as time \
        from records r ",
    );

    builder
        .push_event_join(&mut q, "eer", "r")
        .push(" where map_id = ")
        .push_bind(ctx.get_map_id())
        .push(" ");
    builder
        .push_event_filter(&mut q, "eer")
        .push("group by record_player_id order by time, record_player_id asc");

    q
}

/// Updates the Redis leaderboard for a map.
///
/// This function deletes the Redis key and reinserts all the records from the MariaDB database.
/// All this is done in a single transaction.
pub async fn force_update<C: HasMapId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> RecordsResult<()> {
    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition()).to_string();

    pipe.del(&key);

    get_mariadb_lb_query(&ctx)
        .build_query_as()
        .fetch(&mut **conn.mysql_conn)
        .map_ok(|(player_id, time): (u32, i32)| {
            pipe.zadd(&key, player_id, time);
        })
        .try_collect::<()>()
        .await?;

    let _: () = pipe.query_async(conn.redis_conn).await?;

    Ok(())
}

async fn get_rank_impl(
    redis_conn: &mut RedisConnection,
    key: &MapKey<'_>,
    time: i32,
) -> RecordsResult<Option<i32>> {
    let player_ids: Vec<u32> = redis_conn
        .zrangebyscore_limit(key, time, time, 0, 1)
        .await?;

    match player_ids.first() {
        Some(id) => {
            let rank: Option<i32> = redis_conn.zrank(key, *id).await?;
            Ok(rank.map(|rank| rank + 1))
        }
        None => Ok(None),
    }
}

/// Gets the rank of a player in a map, or fully updates its leaderboard if not found.
///
/// The full update means a delete of the Redis key then a reinsertion of all the records.
/// This may be called when the SQL and Redis databases had the same amount of records on a map,
/// but the times were not corresponding. It generally happens after a database migration.
///
/// The ranking type is the standard competition ranking (1224).
///
/// See the [module documentation](super) for more information.
pub async fn get_rank<C>(db: &mut DatabaseConnection<'_>, ctx: C, time: i32) -> RecordsResult<i32>
where
    C: HasMapId + HasPlayerId,
{
    let key = map_key(ctx.get_map_id(), ctx.get_opt_event_edition());

    // Get the time of the player saved in Redis and update it if inconsistent
    let score: Option<i32> = db.redis_conn.zscore(&key, ctx.get_player_id()).await?;
    if score.is_none_or(|t| t != time) {
        force_update(db, &ctx).await?;
    }

    match get_rank_impl(db.redis_conn, &key, time).await? {
        Some(r) => Ok(r),
        None => Err(get_rank_failed(db, ctx, time).await?),
    }
}

/// Panics with a clear message of the leaderboards differences between MariaDB and Redis.
///
/// The `O` generic parameter is used to return the same type as the [`get_rank`] function.
#[cold]
async fn get_rank_failed<C>(
    db: &mut DatabaseConnection<'_>,
    ctx: C,
    time: i32,
) -> RecordsResult<RecordsError>
where
    C: HasPlayerId + HasMapId,
{
    use std::fmt::Write as _;

    fn num_digits<N>(n: N) -> usize
    where
        f64: From<N>,
    {
        (f64::from(n).log10() + 1.) as _
    }

    let event = ctx.get_opt_event_edition();
    let player_id = ctx.get_player_id();
    let map_id = ctx.get_map_id();

    let key = &map_key(ctx.get_map_id(), event);
    let redis_lb: Vec<i64> = db.redis_conn.zrange_withscores(key, 0, -1).await?;

    let mariadb_lb = get_mariadb_lb_query(&ctx)
        .build_query_as::<(u32, i32)>()
        .fetch_all(&mut **db.mysql_conn)
        .await?;

    let lb = redis_lb
        .chunks_exact(2)
        .map(|chunk| (chunk[0] as u32, chunk[1] as i32))
        .zip_longest(mariadb_lb)
        .collect::<Vec<_>>();

    let width = lb
        .iter()
        .map(|e| match e {
            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                num_digits(*rpid) + num_digits(*rtime) + num_digits(*mpid) + num_digits(*mtime)
            }
            EitherOrBoth::Left((rpid, rtime)) => num_digits(*rpid) + num_digits(*rtime),
            EitherOrBoth::Right((mpid, mtime)) => num_digits(*mpid) + num_digits(*mtime),
        })
        .max()
        .unwrap_or_default()
        .max("player".len() * 2 + "time".len() * 2 + 1);
    let w4 = width / 4;

    let mut msg = format!(
        "{:w2$} || {:w2$}\n{empty:-<w$}\n{player:w4$} | {time:w4$} || {player:w4$} | {time:w4$}\n",
        "redis",
        "mariadb",
        player = "player",
        time = "time",
        w2 = width / 2 + 3,
        w = width + 10,
        empty = "",
    );

    for row in lb {
        match row {
            EitherOrBoth::Both((rpid, rtime), (mpid, mtime)) => {
                writeln!(
                    msg,
                    "{c}{rpid:w4$} | {rtime:w4$} || {mpid:w4$} | {mtime:w4$}{c_end}",
                    c = if rpid != mpid || rtime != mtime {
                        "\x1b[93m"
                    } else if player_id == rpid && player_id == mpid {
                        "\x1b[34m"
                    } else {
                        ""
                    },
                    c_end = if rpid != mpid
                        || rtime != mtime
                        || (player_id == rpid && player_id == mpid)
                    {
                        "\x1b[0m"
                    } else {
                        ""
                    },
                )
                .unwrap();
            }
            EitherOrBoth::Left((rpid, rtime)) => {
                writeln!(
                    msg,
                    "\x1b[93m{rpid:w4$} | {rtime:w4$} || {empty:w4$} | {empty:w4$}\x1b[0m",
                    empty = ""
                )
                .unwrap();
            }
            EitherOrBoth::Right((mpid, mtime)) => {
                writeln!(
                    msg,
                    "\x1b[93m{empty:w4$} | {empty:w4$} || {mpid:w4$} | {mtime:w4$}\x1b[0m",
                    empty = ""
                )
                .unwrap();
            }
        }
    }

    #[cfg(feature = "tracing")]
    tracing::error!("missing player rank ({player_id} on map {map_id} with time {time})\n{msg}");

    Ok(RecordsError::Internal)
}
