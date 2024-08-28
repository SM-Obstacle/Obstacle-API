use std::future::Future;

use deadpool_redis::redis::AsyncCommands;
use rand::Rng as _;
use records_lib::{models, redis_key::map_key, Database};

use crate::utils::generate_token;

pub async fn with_map_uid<F, Fut, R>(db: Database, f: F) -> anyhow::Result<R>
where
    F: FnOnce(models::Map) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let cps_number = rand::thread_rng().gen_range(0..=90);
    let game_id = generate_token(27);

    let map = models::Map {
        id: 0,
        game_id,
        player_id: 1056, // ahmad's ID
        name: "".to_owned(),
        cps_number: Some(cps_number),
        linked_map: None,
    };

    let id = sqlx::query_scalar(
        "insert into maps (game_id, player_id, name, cps_number, linked_map)
        values (?, ?, ?, ?, ?) returning id",
    )
    .bind(&map.game_id)
    .bind(map.player_id)
    .bind(&map.name)
    .bind(map.cps_number)
    .bind(map.linked_map)
    .fetch_one(&db.mysql_pool)
    .await?;

    let map = models::Map { id, ..map };

    tracing::debug!("using map: {map:#?}");

    let out = f(map).await?;

    tracing::debug!("removing map from db...");

    sqlx::query("delete from maps where id = ?")
        .bind(id)
        .execute(&db.mysql_pool)
        .await?;

    db.redis_pool
        .get()
        .await?
        .del(map_key(id, Default::default()))
        .await?;

    Ok(out)
}
