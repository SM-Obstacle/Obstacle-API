//! This module contains various utility items to retrieve leaderboards information.

use deadpool_redis::redis::AsyncCommands as _;
use entity::{event_edition_records, players, records};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait as _, FromQueryResult, Order, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, QueryTrait as _, StreamTrait, prelude::Expr,
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

    let result = records::Entity::find()
        .inner_join(players::Entity)
        .filter(
            records::Column::MapId
                .eq(map_id)
                .and(records::Column::RecordPlayerId.is_in(player_ids)),
        )
        .group_by(records::Column::RecordPlayerId)
        .order_by(records::Column::Time.min(), Order::Asc)
        .apply_if(event.event, |query, (ev, ed)| {
            query.reverse_join(event_edition_records::Entity).filter(
                event_edition_records::Column::EventId
                    .eq(ev.id)
                    .and(event_edition_records::Column::EditionId.eq(ed.id)),
            )
        })
        .select_only()
        .column_as(records::Column::RecordPlayerId, "player_id")
        .column_as(players::Column::Login, "login")
        .column_as(players::Column::Name, "nickname")
        .column_as(Expr::col(records::Column::Time).min(), "time")
        .column_as(records::Column::MapId, "map_id")
        .into_model::<RecordQueryRow>()
        .all(conn)
        .await?;

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
