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
    ActiveValue::Set, ColumnTrait as _, ConnectionTrait, EntityTrait, QueryFilter, TransactionTrait,
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

    sync::transaction(conn, async |txn| {
        for (player, score) in scores.player_scores {
            players::Entity::update(players::ActiveModel {
                score: Set(score),
                ..Default::default()
            })
            .filter(players::Column::Id.eq(player.inner.id))
            .exec(txn)
            .await?;

            pipe.zadd(player_ranking(), player.inner.id, score);
        }

        for (map, score) in scores.map_scores {
            maps::Entity::update(maps::ActiveModel {
                score: Set(score),
                ..Default::default()
            })
            .filter(maps::Column::Id.eq(map.inner.id))
            .exec(txn)
            .await?;

            pipe.zadd(map_ranking(), map.inner.id, score);
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
    do_update(&db.sql_conn, &db.redis_pool, from).await?;

    tracing::info!("Player and map ranking update completed");

    Ok(())
}
