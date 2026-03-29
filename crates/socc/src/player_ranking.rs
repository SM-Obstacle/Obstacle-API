use anyhow::Context as _;
use chrono::{DateTime, Utc};
use deadpool_redis::redis;
use entity::{maps, players};
use player_map_ranking::compute_scores;
use records_lib::{
    Database, RedisPool,
    redis_key::{map_ranking, player_ranking},
    sync,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter, TransactionTrait, prelude::Expr,
    sea_query::CaseStatement,
};

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

    // To make the bulk update in SQL, we build a query so it looks like this:
    //
    // UPDATE [ players | maps ]
    // SET score = CASE
    //   WHEN id = :id0 THEN :score0
    //   WHEN id = :id1 THEN :score1
    //   ...
    // WHERE id IN (:id0, :id1, ...);
    //
    // Due to limitations in DB engines such as MariaDB and Postgres, the placeholders limit is fixed
    // to u16::MAX. Therefore, we must chunk our query accordingly. Each item gets 3 placeholders:
    // the score, and the ID twice; so the chunk size is u16::MAX / 3.

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

    let player_updates = player_scores.chunks(u16::MAX as usize / 3).map(|chunk| {
        (
            chunk.iter().map(|(player, _)| player.inner.id),
            chunk
                .iter()
                .fold(CaseStatement::new(), |case_stmt, (player, score)| {
                    case_stmt.case(
                        Expr::col((players::Entity, players::Column::Id)).eq(player.inner.id),
                        *score,
                    )
                }),
        )
    });

    let map_updates = map_scores.chunks(u16::MAX as usize / 3).map(|chunk| {
        (
            chunk.iter().map(|(map, _)| map.inner.id),
            chunk
                .iter()
                .fold(CaseStatement::new(), |case_stmt, (map, score)| {
                    case_stmt.case(
                        Expr::col((maps::Entity, maps::Column::Id)).eq(map.inner.id),
                        *score,
                    )
                }),
        )
    });

    sync::transaction(conn, async |txn| {
        for (player_ids, case_stmt) in player_updates {
            players::Entity::update_many()
                .col_expr(players::Column::Score, case_stmt.into())
                .filter(players::Column::Id.is_in(player_ids))
                .exec(txn)
                .await?;
        }

        for (map_ids, case_stmt) in map_updates {
            maps::Entity::update_many()
                .col_expr(maps::Column::Score, case_stmt.into())
                .filter(maps::Column::Id.is_in(map_ids))
                .exec(txn)
                .await?;
        }

        anyhow::Ok(())
    })
    .await?;

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
