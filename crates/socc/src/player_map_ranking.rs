use std::{collections::HashMap, time::Duration};

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use deadpool_redis::redis;
use entity::{map_periodic_ranking, maps, player_periodic_ranking, players, ranking_period};
use futures::future::try_join_all;
use player_map_ranking::compute_scores;
use records_lib::{
    Database, RedisPool,
    redis_key::{map_ranking, player_ranking},
    sync,
};
use sea_orm::{
    ActiveValue::Set,
    ColumnTrait as _, ConnectionTrait, DbErr, DeriveIden, EntityName, EntityTrait,
    QueryFilter as _, QueryOrder as _, QuerySelect as _, TransactionTrait,
    sea_query::{ColumnDef, ForeignKey, Index, IntoIden, IntoTableRef, Query, SimpleExpr, Table},
};
use tokio::time;

trait TempTable {
    fn table() -> impl IntoTableRef;
    fn ref_table() -> impl IntoTableRef;
    fn id() -> impl IntoIden;
    fn ref_id() -> impl IntoIden;
    fn score() -> impl IntoIden;
}

#[derive(DeriveIden)]
enum PlayerScoreTemp {
    Table,
    PlayerId,
    Score,
}

#[derive(DeriveIden)]
enum MapScoreTemp {
    Table,
    MapId,
    Score,
}

struct TempPlayerScores;
struct TempMapScores;

impl TempTable for TempPlayerScores {
    fn table() -> impl IntoTableRef {
        PlayerScoreTemp::Table
    }

    fn ref_table() -> impl IntoTableRef {
        players::Entity.table_ref()
    }

    fn id() -> impl IntoIden {
        PlayerScoreTemp::PlayerId
    }

    fn ref_id() -> impl IntoIden {
        players::Column::Id
    }

    fn score() -> impl IntoIden {
        PlayerScoreTemp::Score
    }
}

impl TempTable for TempMapScores {
    fn table() -> impl IntoTableRef {
        MapScoreTemp::Table
    }

    fn ref_table() -> impl IntoTableRef {
        maps::Entity.table_ref()
    }

    fn id() -> impl IntoIden {
        MapScoreTemp::MapId
    }

    fn ref_id() -> impl IntoIden {
        maps::Column::Id
    }

    fn score() -> impl IntoIden {
        MapScoreTemp::Score
    }
}

async fn with_temp_table<T, O, E, F, C>(conn: &C, _temp_table: T, f: F) -> Result<O, E>
where
    F: AsyncFnOnce() -> Result<O, E>,
    C: ConnectionTrait,
    E: From<DbErr>,
    T: TempTable,
{
    let create_stmt = conn.get_database_backend().build(
        Table::create()
            .table(T::table())
            .col(ColumnDef::new(T::id()).unsigned().not_null())
            .col(ColumnDef::new(T::score()).double().not_null())
            .foreign_key(
                ForeignKey::create()
                    .from(T::table(), T::id())
                    .to(T::ref_table(), T::ref_id())
                    .on_delete(sea_orm::sea_query::ForeignKeyAction::Cascade),
            )
            .primary_key(Index::create().col(T::id())),
    );
    println!("ok stmt:\n{}", create_stmt.sql);
    let drop_stmt = conn
        .get_database_backend()
        .build(Table::drop().table(T::table()));

    conn.execute(create_stmt).await?;

    let ret = f().await;

    conn.execute(drop_stmt).await?;

    ret
}

const CHUNK_SIZE: usize = 1000;

/// Returns the scores of the players and maps of the last global ranking period, if any.
async fn get_last_global_scores<C: ConnectionTrait>(
    conn: &C,
) -> Result<Option<(HashMap<u32, f64>, HashMap<u32, f64>)>, DbErr> {
    let Some(period_id) = ranking_period::Entity::find()
        .select_only()
        .column(ranking_period::Column::PeriodId)
        .filter(ranking_period::Column::IsGlobal.eq(true))
        .order_by_desc(ranking_period::Column::PeriodDate)
        .limit(1)
        .into_tuple::<i32>()
        .one(conn)
        .await?
    else {
        return Ok(None);
    };

    let player_scores = player_periodic_ranking::Entity::find()
        .select_only()
        .columns([
            player_periodic_ranking::Column::PlayerId,
            player_periodic_ranking::Column::Score,
        ])
        .filter(player_periodic_ranking::Column::PeriodId.eq(period_id))
        .into_tuple::<(u32, f64)>()
        .all(conn)
        .await?
        .into_iter()
        .collect();

    let map_scores = map_periodic_ranking::Entity::find()
        .select_only()
        .columns([
            map_periodic_ranking::Column::MapId,
            map_periodic_ranking::Column::Score,
        ])
        .filter(map_periodic_ranking::Column::PeriodId.eq(period_id))
        .into_tuple::<(u32, f64)>()
        .all(conn)
        .await?
        .into_iter()
        .collect();

    Ok(Some((player_scores, map_scores)))
}

/// Combines the scores of a periodic ranking with the global scores, in tiers.
///
/// The players/maps that were active during the period (having a periodic score above 0) are
/// ranked first, by their periodic score. The other ones are ranked below, by their global score.
///
/// To keep a single sortable score (for both Redis and the DB), the active ones get their
/// periodic score offset by the highest global score, plus one.
fn tier_scores<K>(
    scores: impl IntoIterator<Item = (K, f64)>,
    id: impl Fn(&K) -> u32,
    global_scores: &HashMap<u32, f64>,
) -> Vec<(K, f64)> {
    let offset = global_scores.values().copied().fold(0., f64::max) + 1.;

    scores
        .into_iter()
        .map(|(key, score)| {
            let score = if score > 0. {
                score + offset
            } else {
                global_scores.get(&id(&key)).copied().unwrap_or(0.)
            };
            (key, score)
        })
        .collect()
}

/// Computes and saves the player/map ranking scores.
///
/// If `from` is `None`, the scores are computed from all the records, and saved as a global
/// ranking period. These scores aren't saved to Redis.
///
/// Otherwise, the scores are computed from the records made since `from`, then combined with the
/// last global scores (see [`tier_scores`]), and saved both to the DB and to Redis.
async fn do_update<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    from: Option<DateTime<Utc>>,
) -> anyhow::Result<()> {
    let is_global = from.is_none();

    let scores = compute_scores(conn, from)
        .await
        .context("couldn't compute the scores")?;

    let (player_scores, map_scores) = if is_global {
        (
            scores.player_scores.into_iter().collect::<Vec<_>>(),
            scores.map_scores.into_iter().collect::<Vec<_>>(),
        )
    } else {
        let (global_player_scores, global_map_scores) = get_last_global_scores(conn)
            .await
            .context("couldn't get the last global scores")?
            .unwrap_or_else(|| {
                tracing::warn!(
                    "No global ranking period found, inactive players and maps will have a score of 0"
                );
                Default::default()
            });

        (
            tier_scores(
                scores.player_scores,
                |player| player.inner.id,
                &global_player_scores,
            ),
            tier_scores(scores.map_scores, |map| map.inner.id, &global_map_scores),
        )
    };

    let redis_pipe = if is_global {
        None
    } else {
        let mut pipe = redis::pipe();
        pipe.atomic();
        for (player, score) in &player_scores {
            pipe.zadd(player_ranking(), player.inner.id, *score);
        }
        for (map, score) in &map_scores {
            pipe.zadd(map_ranking(), map.inner.id, *score);
        }
        Some(pipe)
    };

    let player_updates = player_scores.chunks(CHUNK_SIZE).map(|chunk| {
        conn.get_database_backend().build(
            &*chunk.iter().fold(
                Query::insert()
                    .into_table(PlayerScoreTemp::Table)
                    .columns([PlayerScoreTemp::PlayerId, PlayerScoreTemp::Score]),
                |insert_stmt, (player, score)| {
                    insert_stmt
                        .values([SimpleExpr::from(player.inner.id), SimpleExpr::from(*score)])
                        .expect("columns should match here")
                },
            ),
        )
    });

    let map_updates = map_scores.chunks(CHUNK_SIZE).map(|chunk| {
        conn.get_database_backend().build(
            &*chunk.iter().fold(
                Query::insert()
                    .into_table(MapScoreTemp::Table)
                    .columns([MapScoreTemp::MapId, MapScoreTemp::Score]),
                |insert_stmt, (map, score)| {
                    insert_stmt
                        .values([SimpleExpr::from(map.inner.id), SimpleExpr::from(*score)])
                        .expect("columns should match here")
                },
            ),
        )
    });

    let res: Result<_, DbErr> = sync::transaction(conn, async |txn| {
        let new_period = ranking_period::ActiveModel {
            period_date: Set(chrono::Utc::now().naive_utc()),
            is_global: Set(is_global),
            ..Default::default()
        };
        let new_period_id = ranking_period::Entity::insert(new_period)
            .exec(txn)
            .await?
            .last_insert_id;

        with_temp_table(txn, TempPlayerScores, async || {
            try_join_all(player_updates.map(|stmt| conn.execute(stmt))).await?;

            txn.execute(
                txn.get_database_backend().build(
                    Query::insert()
                        .into_table(player_periodic_ranking::Entity.table_ref())
                        .columns([
                            player_periodic_ranking::Column::PeriodId,
                            player_periodic_ranking::Column::PlayerId,
                            player_periodic_ranking::Column::Score,
                        ])
                        .select_from(
                            Query::select()
                                .from(PlayerScoreTemp::Table)
                                .expr(new_period_id)
                                .column(PlayerScoreTemp::PlayerId)
                                .column(PlayerScoreTemp::Score)
                                .take(),
                        )
                        .unwrap(),
                ),
            )
            .await
        })
        .await?;

        with_temp_table(txn, TempMapScores, async || {
            try_join_all(map_updates.map(|stmt| conn.execute(stmt))).await?;

            txn.execute(
                txn.get_database_backend().build(
                    Query::insert()
                        .into_table(map_periodic_ranking::Entity.table_ref())
                        .columns([
                            map_periodic_ranking::Column::PeriodId,
                            map_periodic_ranking::Column::MapId,
                            map_periodic_ranking::Column::Score,
                        ])
                        .select_from(
                            Query::select()
                                .from(MapScoreTemp::Table)
                                .expr(new_period_id)
                                .column(MapScoreTemp::MapId)
                                .column(MapScoreTemp::Score)
                                .take(),
                        )
                        .unwrap(),
                ),
            )
            .await
        })
        .await?;

        Ok(())
    })
    .await;
    res?;

    if let Some(pipe) = redis_pipe {
        let mut redis_conn = redis_pool
            .get()
            .await
            .context("couldn't get redis connection")?;
        pipe.exec_async(&mut redis_conn)
            .await
            .context("couldn't save scores to Redis")?;
    }

    Ok(())
}

async fn update(db: &Database, from: Option<DateTime<Utc>>) -> anyhow::Result<()> {
    let kind = if from.is_none() { "global" } else { "periodic" };
    let res = do_update(&db.sql_conn, &db.redis_pool, from).await;

    match &res {
        Ok(_) => tracing::info!("Player and map {kind} ranking update completed successfully"),
        Err(e) => tracing::error!("Player and map {kind} ranking update returned an error: {e}"),
    }

    res
}

/// Runs the global and periodic player/map ranking updates, each at its own interval.
///
/// Both updates run in the same task, so they never overlap (they share the same intermediate
/// tables). When both are due, the global one runs first, so the periodic one uses fresh global
/// scores, which is notably the case at startup.
pub async fn run(
    db: Database,
    periodic_interval: Duration,
    global_interval: Duration,
) -> anyhow::Result<()> {
    let mut global_ticks = time::interval(global_interval);
    let mut periodic_ticks = time::interval(periodic_interval);

    loop {
        tokio::select! {
            biased;
            _ = global_ticks.tick() => update(&db, None).await?,
            _ = periodic_ticks.tick() => {
                update(&db, Some(chrono::Utc::now() - periodic_interval)).await?
            }
        }
    }
}
