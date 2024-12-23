//! This module contains various utility items to retrieve leaderboards information.

use std::fmt;

use sqlx::QueryBuilder;

use crate::{
    context::{Ctx, HasMapId, HasPersistentMode, ReadOnly, Transactional},
    error::RecordsResult,
    ranks::get_rank,
    transaction, DatabaseConnection, RedisConnection,
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

struct LeaderboardImplParam<'a> {
    redis_conn: &'a mut RedisConnection,
    offset: Option<isize>,
    limit: Option<isize>,
}

async fn leaderboard_impl<C>(
    mysql_conn: &mut sqlx::pool::PoolConnection<sqlx::MySql>,
    ctx: C,
    LeaderboardImplParam {
        redis_conn,
        offset,
        limit,
    }: LeaderboardImplParam<'_>,
) -> RecordsResult<Vec<Row>>
where
    C: HasMapId + Transactional,
{
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

    let conn = &mut DatabaseConnection {
        redis_conn,
        mysql_conn,
    };

    let builder = ctx.sql_frag_builder();

    let mut query = QueryBuilder::new(
        "select
            p.id as player_id,
            p.login as player,
            r.time as time
        from ",
    );
    builder
        .push_event_view_name(&mut query, "r")
        .push(
            " r inner join players p on p.id = r.record_player_id \
        where r.map_id = ",
        )
        .push_bind(ctx.get_map_id())
        .push(" ");
    let query = builder
        .push_event_filter(&mut query, "r")
        .push(" order by r.time ")
        .push(Limit(limit))
        .push(" ")
        .push(Offset(offset))
        .build_query_as();

    let rows: Vec<RawRow> = query.fetch_all(&mut **conn.mysql_conn).await?;

    let mut out = Vec::with_capacity(rows.len());

    for row in rows {
        let ctx = Ctx::with_player_id(&ctx, row.player_id);
        let rank = get_rank(conn, ctx, row.time).await?;
        out.push(Row {
            rank,
            player: row.player,
            time: row.time,
        });
    }

    Ok(out)
}

/// Returns a list of the leaderboard of a map from its ID.
pub async fn leaderboard<C>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
    offset: Option<isize>,
    limit: Option<isize>,
) -> RecordsResult<Vec<Row>>
where
    C: HasMapId + HasPersistentMode,
{
    transaction::within_transaction(
        conn.mysql_conn,
        ctx,
        ReadOnly,
        LeaderboardImplParam {
            redis_conn: conn.redis_conn,
            offset,
            limit,
        },
        leaderboard_impl,
    )
    .await
}
