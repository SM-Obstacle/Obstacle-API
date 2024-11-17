//! This module contains various utility items to retrieve leaderboards information.

use std::fmt;

use futures::{Stream, TryStreamExt};

use crate::{error::RecordsResult, event::OptEvent, ranks::get_rank, Database};

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

#[derive(sqlx::FromRow)]
struct RawRow {
    player_id: u32,
    player: String,
    time: i32,
}

/// The type yielded by the [`leaderboard`] function.
pub struct Row {
    /// The rank of the record.
    pub rank: i32,
    /// The login of the player.
    pub player: String,
    /// The time of the record.
    pub time: i32,
}

/// Returns the SQL query to be used as the `query` parameter of the [`leaderboard`] function.
pub fn sql_query(event: OptEvent<'_, '_>, offset: Option<isize>, limit: Option<isize>) -> String {
    struct Offset(Option<isize>);

    impl fmt::Display for Offset {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Some(offset) => write!(f, "offset {offset}"),
                None => Ok(()),
            }
        }
    }

    struct Limit(Option<isize>);

    impl fmt::Display for Limit {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Some(limit) => write!(f, "limit {limit}"),
                None => Ok(()),
            }
        }
    }

    let (view_name, and_event) = event.get_view();

    format!(
        "select
            p.id as player_id,
            p.login as player,
            r.time as time
        from {view_name} r
        inner join players p on p.id = r.record_player_id
        where r.map_id = ? {and_event}
        order by r.time
        {limit} {offset}",
        limit = Limit(limit),
        offset = Offset(offset),
    )
}

/// Returns a stream of the leaderboard of a map from its ID.
///
/// The `query` parameter must be a call to [`sql_query`].
///
/// > The signature of this function is likely to change.
///
/// ## Example
///
/// ```no_run
/// # use records_lib::leaderboard as lb;
/// # use futures::TryStreamExt as _;
/// # #[tokio::main] async fn main() {
/// # let env = <records_lib::DbEnv as mkenv::Env>::get();
/// # let db = records_lib::Database {
/// #   mysql_pool: records_lib::get_mysql_pool(env.db_url.db_url).await.unwrap(),
/// #   redis_pool: records_lib::get_redis_pool(env.redis_url.redis_url).unwrap(),
/// # };
/// let lb = lb::leaderboard(
///     &db,
///     60830,
///     Default::default(),
///     &lb::sql_query(Default::default(), None, None)
/// )
/// .try_collect::<Vec<_>>()
/// .await;
/// # }
/// ```
pub fn leaderboard<'a>(
    db: &'a Database,
    map_id: u32,
    event: OptEvent<'a, 'a>,
    query: &'a str,
) -> impl Stream<Item = RecordsResult<Row>> + 'a {
    sqlx::query_as::<_, RawRow>(query)
        .bind(map_id)
        .fetch(&db.mysql_pool)
        .map_err(From::from)
        .and_then(move |row| async move {
            let mut conn = db.acquire().await?;
            let rank = get_rank(&mut conn, map_id, row.player_id, row.time, event).await?;
            Ok(Row {
                rank,
                player: row.player,
                time: row.time,
            })
        })
}
