//! This module contains various utility items to retrieve leaderboards information.

use deadpool_redis::redis::AsyncCommands as _;
use entity::{global_event_records, global_records, players, records};
use sea_orm::{
    ConnectionTrait, FromQueryResult, Order, StatementBuilder, StreamTrait, prelude::Expr,
    sea_query::Query,
};

use crate::{
    RedisConnection, error::RecordsResult, opt_event::OptEvent, ranks, redis_key::map_key,
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

#[derive(Debug, Clone, FromQueryResult)]
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
pub async fn leaderboard_into<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    start: Option<i32>,
    end: Option<i32>,
    rows: &mut Vec<Row>,
    event: OptEvent<'_>,
) -> RecordsResult<()> {
    ranks::update_leaderboard(conn, redis_conn, map_id, event).await?;

    let start = start.unwrap_or_default();
    let end = end.unwrap_or(-1);
    let player_ids: Vec<i32> = redis_conn
        .zrange(map_key(map_id, event), start as isize, end as isize)
        .await?;

    if player_ids.is_empty() {
        return Ok(());
    }

    let mut query = Query::select();
    query
        .join_as(
            sea_orm::JoinType::InnerJoin,
            players::Entity,
            "p",
            Expr::col(("r", records::Column::RecordPlayerId))
                .eq(Expr::col(("p", players::Column::Id))),
        )
        .and_where(
            Expr::col(("r", records::Column::MapId))
                .eq(map_id)
                .and(Expr::col(("p", players::Column::Id)).is_in(player_ids)),
        )
        .order_by_expr(Expr::col(("r", records::Column::Time)).into(), Order::Asc)
        .order_by_expr(
            Expr::col(("r", records::Column::RecordDate)).into(),
            Order::Asc,
        )
        .expr_as(Expr::col(("p", players::Column::Id)), "player_id")
        .expr_as(Expr::col(("p", players::Column::Login)), "login")
        .expr_as(Expr::col(("p", players::Column::Name)), "nickname")
        .expr_as(Expr::col(("r", records::Column::Time)), "time")
        .expr_as(Expr::col(("r", records::Column::MapId)), "map_id");

    match event.event {
        Some((ev, ed)) => {
            query.from_as(global_event_records::Entity, "r").and_where(
                Expr::col(("r", global_event_records::Column::EventId))
                    .eq(ev.id)
                    .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
            );
        }
        None => {
            query.from_as(global_records::Entity, "r");
        }
    }

    let stmt = StatementBuilder::build(&query, &conn.get_database_backend());
    let result = conn
        .query_all(stmt)
        .await?
        .into_iter()
        .map(|result| RecordQueryRow::from_query_result(&result, ""))
        .collect::<Result<Vec<_>, _>>()?;

    rows.reserve(result.len());

    for r in result {
        rows.push(Row {
            rank: ranks::get_rank(conn, redis_conn, map_id, r.player_id, r.time, event).await?,
            login: r.login,
            nickname: r.nickname,
            time: r.time,
        });
    }

    Ok(())
}

/// Returns the leaderboard of a map.
pub async fn leaderboard<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    start: Option<i32>,
    end: Option<i32>,
    event: OptEvent<'_>,
) -> RecordsResult<Vec<Row>> {
    let mut out = Vec::new();
    leaderboard_into(conn, redis_conn, map_id, start, end, &mut out, event).await?;
    Ok(out)
}
