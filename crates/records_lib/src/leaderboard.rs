//! This module contains various utility items to retrieve leaderboards information.

use deadpool_redis::redis::AsyncCommands as _;

use crate::{
    DatabaseConnection,
    error::RecordsResult,
    opt_event::OptEvent,
    ranks,
    redis_key::map_key,
    transaction::{self, ReadOnly, TxnGuard},
};

/// The type returned by the [`compet_rank_by_key`](CompetRankingByKeyIter::compet_rank_by_key)
/// method.
pub struct CompetitionRankingByKey<I, K, F> {
    iter: I,
    func: F,

    previous_key: Option<K>,
    rank: usize,
    offset: usize,
}

impl<I, K, F> CompetitionRankingByKey<I, K, F> {
    fn new(iter: I, func: F) -> Self {
        Self {
            iter,
            func,

            previous_key: None,
            rank: 0,
            offset: 1,
        }
    }
}

impl<I, K, F> Iterator for CompetitionRankingByKey<I, K, F>
where
    I: Iterator,
    F: FnMut(&<I as Iterator>::Item) -> K,
    K: Eq,
{
    type Item = (usize, <I as Iterator>::Item);

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next()?;
        let key = (self.func)(&next);

        match self.previous_key.take() {
            // First iteration
            None => {
                self.rank += 1;
            }
            // Same keys
            Some(previous_key) if previous_key == key => {
                self.offset += 1;
            }
            // Differents keys
            _ => {
                self.rank += self.offset;
                self.offset = 1;
            }
        }

        self.previous_key = Some(key);

        Some((self.rank, next))
    }
}

/// Extends the [`Iterator`] trait by providing the [`compet_rank_by_key`][1] method.
///
/// [1]: CompetRankingByKeyIter::compet_rank_by_key
pub trait CompetRankingByKeyIter: Iterator {
    /// Maps each item to a rank respecting the competition ranking system (1224).
    ///
    /// The key returned by the function is used to know when to increase the rank.
    ///
    /// ## Example
    ///
    /// ```
    /// # use records_lib::leaderboard::CompetRankingByKeyIter as _;
    /// let scores = vec![30, 31, 31, 33, 34, 34, 34, 35, 36]
    ///     .into_iter()
    ///     .compet_rank_by_key(|i| *i)
    ///     .collect::<Vec<_>>();
    ///
    /// assert_eq!(
    ///     scores,
    ///     vec![
    ///         (1, 30),
    ///         (2, 31),
    ///         (2, 31),
    ///         (4, 33),
    ///         (5, 34),
    ///         (5, 34),
    ///         (5, 34),
    ///         (8, 35),
    ///         (9, 36),
    ///     ]
    /// );
    /// ```
    fn compet_rank_by_key<K, F>(self, f: F) -> CompetitionRankingByKey<Self, K, F>
    where
        F: FnMut(&Self::Item) -> K,
        K: Eq,
        Self: Sized,
    {
        CompetitionRankingByKey::new(self, f)
    }
}

impl<I: Iterator> CompetRankingByKeyIter for I {}

#[derive(sqlx::FromRow, Debug, Clone)]
struct RecordQueryRow {
    player_id: u32,
    login: String,
    nickname: String,
    time: i32,
}

/// The type yielded by the [`leaderboard`] function.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Row {
    /// The rank of the record.
    pub rank: i32,
    /// The login of the player.
    pub login: String,
    /// The nickname of the player.
    pub nickname: String,
    /// The time of the record.
    pub time: i32,
}

/// Gets the leaderboard of a map and extends it to the provided vec.
pub async fn leaderboard_txn_into<M>(
    conn: &mut DatabaseConnection<'_>,
    guard: TxnGuard<'_, M>,
    map_id: u32,
    start: Option<i64>,
    end: Option<i64>,
    rows: &mut Vec<Row>,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    ranks::update_leaderboard(conn, map_id, guard, event).await?;

    let start = start.unwrap_or_default();
    let end = end.unwrap_or(-1);
    let player_ids: Vec<i32> = conn
        .redis_conn
        .zrange(map_key(map_id, event), start as isize, end as isize)
        .await?;

    if player_ids.is_empty() {
        return Ok(());
    }

    let builder = event.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new(
        "SELECT p.id AS player_id,
            p.login AS login,
            p.name AS nickname,
            min(time) as time,
            map_id
        FROM records r ",
    );
    builder
        .push_event_join(&mut query, "eer", "r")
        .push(
            " INNER JOIN players p ON r.record_player_id = p.id
            WHERE map_id = ",
        )
        .push_bind(map_id)
        .push(" AND record_player_id IN (");
    let mut sep = query.separated(", ");
    for id in player_ids {
        sep.push_bind(id);
    }
    query.push(") ");
    let result = builder
        .push_event_filter(&mut query, "eer")
        .push(" GROUP BY record_player_id ORDER BY time, record_date ASC")
        .build_query_as::<RecordQueryRow>()
        .fetch_all(&mut **conn.mysql_conn)
        .await?;

    rows.reserve(result.len());

    for r in result {
        rows.push(Row {
            rank: ranks::get_rank(conn, map_id, r.player_id, r.time, event, guard).await?,
            login: r.login,
            nickname: r.nickname,
            time: r.time,
        });
    }

    Ok(())
}

/// Returns the leaderboard of a map.
pub async fn leaderboard_txn<M>(
    conn: &mut DatabaseConnection<'_>,
    guard: TxnGuard<'_, M>,
    map_id: u32,
    start: Option<i64>,
    end: Option<i64>,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<Row>> {
    let mut out = Vec::new();
    leaderboard_txn_into(conn, guard, map_id, start, end, &mut out, event).await?;
    Ok(out)
}

/// Returns the leaderboard of a map.
///
/// This function simply makes a transaction and returns the result of
/// the [`leaderboard_txn`]. function.
pub async fn leaderboard(
    conn: &mut DatabaseConnection<'_>,
    event: OptEvent<'_>,
    map_id: u32,
    offset: Option<i64>,
    limit: Option<i64>,
) -> RecordsResult<Vec<Row>> {
    transaction::within(conn.mysql_conn, ReadOnly, async |mysql_conn, guard| {
        leaderboard_txn(
            &mut DatabaseConnection {
                redis_conn: conn.redis_conn,
                mysql_conn,
            },
            guard,
            map_id,
            offset,
            limit.map(|x| offset.unwrap_or_default() + x),
            event,
        )
        .await
    })
    .await
}
