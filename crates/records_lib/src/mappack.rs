//! This module contains anything related to mappacks in this library.

use std::fmt;

use deadpool_redis::redis::{AsyncCommands, ToRedisArgs};
use entity::{
    event, event_edition, event_edition_maps, global_event_records, global_records, mappack_maps,
    mappacks, maps, players, records,
};
use sea_orm::{
    ActiveValue::Set,
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, Order, QueryFilter, QueryOrder,
    StreamTrait, TransactionTrait,
    prelude::Expr,
    sea_query::{Asterisk, Query},
};

use crate::{
    RedisPool, error::RecordsResult, internal, opt_event::OptEvent, ranks, redis_key::mappacks_key,
    sync,
};

#[derive(Default, Clone, Debug)]
struct Rank {
    rank: i32,
    map_idx: usize,
}

#[derive(Debug)]
struct PlayerScore {
    player_id: u32,
    ranks: Vec<Rank>,
    score: f64,
    maps_finished: usize,
    rank: u32,
    worst: Rank,
}

#[derive(Debug)]
struct MappackMap {
    map_id: u32,
    last_rank: i32,
    records: Option<Vec<RankedRecordRow>>,
}

#[derive(Debug)]
struct MappackScores {
    maps: Vec<MappackMap>,
    scores: Vec<PlayerScore>,
}

#[derive(FromQueryResult, Debug)]
struct RecordRow {
    #[sea_orm(nested)]
    record: records::Model,
    player_id2: u32,
}

#[derive(Debug)]
struct RankedRecordRow {
    rank: i32,
    record: RecordRow,
}

/// Represents any mappack ID, meaning an event or a regular MX mappack.
///
/// If it is an event without an associated mappack, the mappack ID is `__X__Y__` where X
/// is the event ID and Y the edition ID. Otherwise, it is the ID of the associated mappack.
///
/// If it is a regular MX mappack, it is its ID.
#[derive(Clone, Copy)]
pub enum AnyMappackId<'a> {
    /// The mappack is related to an event.
    Event(&'a event::Model, &'a event_edition::Model),
    /// The mappack is a regular MX mappack.
    Id(&'a str),
}

impl fmt::Debug for AnyMappackId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.mappack_id(), f)
    }
}

/// Wrapper type of the [`AnyMappackId`] type to be displayed as a mappack ID.
pub struct MappackIdDisp<'a, 'b> {
    mappack_id: &'a AnyMappackId<'b>,
}

impl fmt::Display for MappackIdDisp<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.mappack_id {
            AnyMappackId::Event(_, edition) => {
                if let Some(id) = edition.mx_id {
                    fmt::Display::fmt(&id, f)
                } else {
                    write!(f, "__{}__{}__", edition.event_id, edition.id)
                }
            }
            AnyMappackId::Id(id) => f.write_str(id),
        }
    }
}

impl ToRedisArgs for MappackIdDisp<'_, '_> {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + deadpool_redis::redis::RedisWrite,
    {
        out.write_arg_fmt(self)
    }
}

impl AnyMappackId<'_> {
    /// Returns a displayable version of the mappack ID.
    pub fn mappack_id(&self) -> MappackIdDisp<'_, '_> {
        MappackIdDisp { mappack_id: self }
    }

    /// Returns whether the mappack has a time-to-live or not.
    ///
    /// Only regular MX mappacks have a time-to-live.
    fn has_ttl(&self) -> bool {
        matches!(self, Self::Id(_))
    }
}

/// Calculates the scores of the players on the provided mappack, and save the results
/// in the SQL database.
#[cfg_attr(
    feature = "tracing",
    tracing::instrument(skip(conn, redis_pool), fields(mappack = %mappack.mappack_id()), err)
)]
pub async fn update_mappack<C: TransactionTrait + ConnectionTrait + Sync>(
    conn: &C,
    redis_pool: &RedisPool,
    mappack: AnyMappackId<'_>,
    event: OptEvent<'_>,
) -> RecordsResult<usize> {
    ensure_mappack(conn, mappack).await?;
    // Calculate the scores
    let scores = crate::assert_future_send(sync::transaction_with_config(
        conn,
        Some(sea_orm::IsolationLevel::RepeatableRead),
        Some(sea_orm::AccessMode::ReadOnly),
        async |txn| calc_scores(txn, redis_pool, mappack, event).await,
    ))
    .await?;

    // Early return if the mappack has expired
    let Some(scores) = scores else {
        return Ok(0);
    };

    let total_scores = scores.scores.len();

    save(conn, mappack, scores).await?;

    // And we save it to the registered mappacks set.
    if mappack.has_ttl() {
        let mut redis_conn = redis_pool.get().await?;

        // The mappack has a TTL, so its member will be removed from the set when
        // attempting to retrieve its maps.
        let _: () = redis_conn
            .sadd(mappacks_key(), mappack.mappack_id())
            .await?;
    }

    Ok(total_scores)
}

async fn ensure_mappack<C: ConnectionTrait>(
    conn: &C,
    mappack: AnyMappackId<'_>,
) -> RecordsResult<()> {
    let mappack_id = mappack.mappack_id().to_string();
    mappacks::Entity::insert(mappacks::ActiveModel {
        id: Set(mappack_id.clone()),
        ..Default::default()
    })
    .on_conflict_do_nothing()
    .exec(conn)
    .await?;
    if let AnyMappackId::Event(event, edition) = mappack {
        let maps = event_edition_maps::Entity::find()
            .filter(
                event_edition_maps::Column::EventId
                    .eq(event.id)
                    .and(event_edition_maps::Column::EditionId.eq(edition.id)),
            )
            .all(conn)
            .await?;
        mappack_maps::Entity::delete_many()
            .filter(mappack_maps::Column::MappackId.eq(&mappack_id))
            .exec(conn)
            .await?;
        let rows = maps
            .into_iter()
            .enumerate()
            .map(|(order, map)| mappack_maps::ActiveModel {
                mappack_id: Set(mappack_id.clone()),
                map_id: Set(map.map_id),
                map_order: Set(order as u32),
            });
        mappack_maps::Entity::insert_many(rows)
            .on_conflict(
                sea_orm::sea_query::OnConflict::columns([
                    mappack_maps::Column::MappackId,
                    mappack_maps::Column::MapId,
                ])
                .update_columns([mappack_maps::Column::MapOrder])
                .to_owned(),
            )
            .exec(conn)
            .await?;
    }
    Ok(())
}

#[cfg_attr(feature = "tracing", tracing::instrument(skip(conn, scores)))]
async fn save<C: ConnectionTrait>(
    conn: &C,
    mappack: AnyMappackId<'_>,
    scores: MappackScores,
) -> RecordsResult<()> {
    let mappack_id = mappack.mappack_id().to_string();
    let (event_id, edition_id) = match mappack {
        AnyMappackId::Event(event, edition) => (Some(event.id), Some(edition.id)),
        AnyMappackId::Id(_) => (None, None),
    };
    mappacks::Entity::insert(mappacks::ActiveModel {
        id: Set(mappack_id.clone()),
        ..Default::default()
    })
    .on_conflict_do_nothing()
    .exec(conn)
    .await?;
    mappacks::Entity::update(mappacks::ActiveModel {
        id: Set(mappack_id.clone()),
        last_updated_at: Set(Some(chrono::Utc::now().naive_utc())),
        ..Default::default()
    })
    .exec(conn)
    .await?;
    let period_id = entity::mappack_ranking_period::Entity::insert(
        entity::mappack_ranking_period::ActiveModel {
            period_date: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        },
    )
    .exec(conn)
    .await?
    .last_insert_id;

    entity::mappack_periodic_ranking::Entity::insert(
        entity::mappack_periodic_ranking::ActiveModel {
            period_id: Set(period_id),
            mappack_id: Set(mappack_id.clone()),
            event_id: Set(event_id),
            edition_id: Set(edition_id),
            maps_count: Set(scores.maps.len() as u32),
        },
    )
    .exec(conn)
    .await?;

    let mut player_rows =
        scores.scores.iter().map(
            |score| entity::mappack_player_periodic_ranking::ActiveModel {
                period_id: Set(period_id),
                mappack_id: Set(mappack_id.clone()),
                player_id: Set(score.player_id),
                rank: Set(score.rank),
                rank_average: Set(((score.score + f64::EPSILON) * 100.).round() / 100.),
                maps_finished: Set(score.maps_finished as u32),
                worst_rank: Set(score.worst.rank as u32),
            },
        );
    if !scores.scores.is_empty() {
        loop {
            let mut chunk = Vec::with_capacity(100);
            for _ in 0..100 {
                match player_rows.next() {
                    Some(row) => chunk.push(row),
                    None => break,
                }
            }
            if chunk.is_empty() {
                break;
            }

            entity::mappack_player_periodic_ranking::Entity::insert_many(chunk)
                .exec(conn)
                .await?;
        }
    }

    let mut map_rows = scores.scores.iter().flat_map(|score| {
        score.ranks.iter().map(|rank| {
            let map = &scores.maps[rank.map_idx];
            entity::mappack_map_periodic_ranking::ActiveModel {
                period_id: Set(period_id),
                mappack_id: Set(mappack_id.clone()),
                map_id: Set(map.map_id),
                player_id: Set(score.player_id),
                rank: Set(rank.rank as u32),
                last_rank: Set(map.last_rank as u32),
            }
        })
    });
    if !scores.scores.is_empty() {
        let chunk_future = crate::assert_future_send(async move {
            loop {
                let mut chunk = Vec::with_capacity(100);
                for _ in 0..100 {
                    match map_rows.next() {
                        Some(row) => chunk.push(row),
                        None => break,
                    }
                }
                if chunk.is_empty() {
                    break;
                }

                entity::mappack_map_periodic_ranking::Entity::insert_many(chunk)
                    .exec(conn)
                    .await?;
            }
            RecordsResult::Ok(())
        });
        chunk_future.await?;
    }

    Ok(())
}

/// Returns an `Option` because the mappack may have expired.
#[cfg_attr(
    feature = "tracing",
    tracing::instrument(skip(conn, redis_pool), fields(mappack = %mappack.mappack_id()))
)]
async fn calc_scores<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    mappack: AnyMappackId<'_>,
    event: OptEvent<'_>,
) -> RecordsResult<Option<MappackScores>> {
    let mut redis_conn = redis_pool.get().await?;
    let mappack_id = mappack.mappack_id().to_string();
    let map_ids = mappack_maps::Entity::find()
        .filter(mappack_maps::Column::MappackId.eq(&mappack_id))
        .order_by_asc(mappack_maps::Column::MapOrder)
        .all(conn)
        .await?;
    if map_ids.is_empty() {
        return Ok(None);
    }
    let mut maps = Vec::with_capacity(map_ids.len().max(5));
    for map_row in map_ids {
        let map = maps::Entity::find_by_id(map_row.map_id)
            .one(conn)
            .await?
            .ok_or_else(|| internal!("map {} should be in database", map_row.map_id))?;
        maps.push(MappackMap {
            map_id: map.id,
            last_rank: 0,
            records: None,
        });
    }

    let mut scores = Vec::<PlayerScore>::with_capacity(maps.len());

    for map in &mut maps {
        let mut query = Query::select();
        query
            .expr(Expr::col(("r", Asterisk)))
            .expr_as(Expr::col(players::Column::Id), "player_id2")
            .expr_as(Expr::col(players::Column::Login), "player_login")
            .expr_as(Expr::col(players::Column::Name), "player_name")
            .join_as(
                sea_orm::JoinType::InnerJoin,
                players::Entity,
                "p",
                Expr::col(("p", players::Column::Id))
                    .eq(Expr::col(("r", records::Column::RecordPlayerId))),
            )
            .and_where(Expr::col(("r", records::Column::MapId)).eq(map.map_id))
            .order_by_expr(Expr::col(("r", records::Column::Time)).into(), Order::Asc);

        match event.get() {
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

        let stmt = conn.get_database_backend().build(&query);
        let res = conn
            .query_all(stmt)
            .await?
            .into_iter()
            .map(|result| RecordRow::from_query_result(&result, ""))
            .collect::<Result<Vec<_>, _>>()?;

        let ranks = ranks::get_ranks(
            conn,
            &mut redis_conn,
            res.iter().map(|record| (map.map_id, record.record.time)),
            event,
        )
        .await?;

        let mut records = Vec::with_capacity(res.len());

        for (record, rank) in res.into_iter().zip(ranks) {
            if !scores.iter().any(|p| p.player_id == record.player_id2) {
                scores.push(PlayerScore {
                    player_id: record.player_id2,
                    ranks: Vec::new(),
                    score: 0.,
                    maps_finished: 0,
                    rank: 0,
                    worst: Default::default(),
                });
            }

            records.push(RankedRecordRow { rank, record });
        }

        map.records = Some(records);
    }

    for (map_idx, map) in maps.iter_mut().enumerate() {
        let map_number = map_idx + 1;
        let records = map.records.take().unwrap();

        let last_rank = records.iter().map(|p| p.rank).max().unwrap_or(0);

        for record in records {
            let player = scores
                .iter_mut()
                .find(|p| p.player_id == record.record.player_id2)
                .unwrap();

            player.ranks.push(Rank {
                rank: record.rank,
                map_idx,
            });
            map.last_rank = last_rank;

            player.maps_finished += 1;
        }

        for player in &mut scores {
            if player.ranks.len() < map_number {
                player.ranks.push(Rank {
                    rank: last_rank + 1,
                    map_idx,
                });
                map.last_rank = last_rank;
            }
        }
    }

    for player in &mut scores {
        player.ranks.sort_by(|a, b| {
            ((a.rank / maps[a.map_idx].last_rank.max(1)
                - b.rank / maps[b.map_idx].last_rank.max(1))
                + (a.rank - b.rank) / 1000)
                .cmp(&0)
        });

        player.worst = player
            .ranks
            .iter()
            .reduce(|a, b| if a.rank > b.rank { a } else { b })
            .unwrap()
            .clone();

        player.score = player
            .ranks
            .iter()
            .fold(0., |acc, rank| acc + rank.rank as f64)
            / player.ranks.len() as f64;
    }

    scores.sort_by(|a, b| {
        if a.maps_finished != b.maps_finished {
            b.maps_finished.cmp(&a.maps_finished)
        } else {
            a.score.partial_cmp(&b.score).unwrap()
        }
    });

    let mut old_score = 0.;
    let mut old_finishes = 0;
    let mut old_rank = 0;

    for (rank, player) in scores.iter_mut().enumerate() {
        player.rank = if old_score.eq(&player.score) && old_finishes == player.maps_finished {
            old_rank
        } else {
            rank as u32 + 1
        };

        old_score = player.score;
        old_finishes = player.maps_finished;
        old_rank = player.rank;
    }

    Ok(Some(MappackScores { maps, scores }))
}
