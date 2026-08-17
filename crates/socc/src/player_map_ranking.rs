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
    ConnectionTrait, DbErr, DeriveIden, EntityName, EntityTrait, TransactionTrait,
    sea_query::{ColumnDef, ForeignKey, Index, IntoIden, IntoTableRef, Query, SimpleExpr, Table},
};

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

async fn do_update<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    from: Option<DateTime<Utc>>,
) -> anyhow::Result<()> {
    let scores = compute_scores(conn, from)
        .await
        .context("couldn't compute the scores")?;

    let mut redis_conn = redis_pool
        .get()
        .await
        .context("couldn't get redis connection")?;

    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    let player_scores = scores
        .player_scores
        .into_iter()
        .map(|(player, score)| {
            pipe.zadd(player_ranking(), player.inner.id, score);
            (player, score)
        })
        .collect::<Vec<_>>();
    let map_scores = scores
        .map_scores
        .into_iter()
        .map(|(map, score)| {
            pipe.zadd(map_ranking(), map.inner.id, score);
            (map, score)
        })
        .collect::<Vec<_>>();

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

    pipe.exec_async(&mut redis_conn)
        .await
        .context("couldn't save scores to Redis")?;

    Ok(())
}

pub async fn update(db: Database, from: Option<DateTime<Utc>>) -> anyhow::Result<()> {
    let res = do_update(&db.sql_conn, &db.redis_pool, from).await;

    match &res {
        Ok(_) => tracing::info!("Player and map ranking update completed successfully"),
        Err(e) => tracing::error!("Player and map ranking update returned an error: {e}"),
    }

    res
}
