use anyhow::Context as _;
use chrono::{DateTime, Utc};
use deadpool_redis::redis::{self, AsyncCommands as _};
use player_map_ranking::compute_scores;
use records_lib::{
    Database, RedisConnection,
    redis_key::{map_ranking, player_ranking},
};
use sea_orm::ConnectionTrait;

async fn do_update<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    from: Option<DateTime<Utc>>,
) -> anyhow::Result<()> {
    let scores = compute_scores(conn, from)
        .await
        .context("couldn't compute the scores")?;

    let mut pipe = redis::pipe();
    let pipe = pipe.atomic();

    for (player, score) in scores.player_scores {
        pipe.zadd(player_ranking(), player.inner.id, score);
    }
    for (map, score) in scores.map_scores {
        pipe.zadd(map_ranking(), map.inner.id, score);
    }
    pipe.exec_async(redis_conn)
        .await
        .context("couldn't save scores to Redis")?;

    Ok(())
}

pub async fn update(db: Database, from: Option<DateTime<Utc>>) -> anyhow::Result<()> {
    let mut redis_conn = db
        .redis_pool
        .get()
        .await
        .context("couldn't get redis connection")?;

    let player_ranking_count: u64 = redis_conn
        .zcount(player_ranking(), "-inf", "+inf")
        .await
        .context("couldn't get player ranking count")?;
    let map_ranking_count: u64 = redis_conn
        .zcount(map_ranking(), "-inf", "+inf")
        .await
        .context("couldn't get map ranking count")?;

    if player_ranking_count == 0 || map_ranking_count == 0 {
        tracing::info!("Empty player or map ranking, doing full update first...");
        do_update(&db.sql_conn, &mut redis_conn, None)
            .await
            .context("couldn't fully update the player and map ranking")?;
    }

    do_update(&db.sql_conn, &mut redis_conn, from).await?;

    tracing::info!("Player and map ranking update completed");

    Ok(())
}
