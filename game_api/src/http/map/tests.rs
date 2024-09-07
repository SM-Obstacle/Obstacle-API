use std::future::Future;

use deadpool_redis::redis::AsyncCommands;
use rand::Rng as _;
use records_lib::{models, redis_key::map_key, Database};

use crate::{
    http::player::tests::{new_player, remove_player},
    utils::generate_token,
};

pub async fn new_map(
    db: Database,
    debug_msg: bool,
) -> anyhow::Result<(models::Map, models::Player)> {
    let player = new_player(&db.mysql_pool, debug_msg).await?;

    let cps_number = rand::thread_rng().gen_range(0..=90);
    let game_id = generate_token(27);

    let map = models::Map {
        id: 0,
        game_id,
        player_id: player.id,
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

    if debug_msg {
        tracing::debug!("using map: {map:#?}");
    }

    Ok((map, player))
}

pub async fn remove_map(db: Database, map_id: u32, debug_msg: bool) -> anyhow::Result<()> {
    if debug_msg {
        tracing::debug!("removing map {map_id} from db...");
    }

    let player_id = sqlx::query_scalar("select player_id from maps where id = ?")
        .bind(map_id)
        .fetch_one(&db.mysql_pool)
        .await?;

    sqlx::query("delete from maps where id = ?")
        .bind(map_id)
        .execute(&db.mysql_pool)
        .await?;

    let _: () = db.redis_pool
        .get()
        .await?
        .del(map_key(map_id, Default::default()))
        .await?;

    remove_player(&db.mysql_pool, player_id, debug_msg).await?;

    Ok(())
}

pub async fn with_map<F, Fut, R>(db: Database, f: F) -> anyhow::Result<R>
where
    F: FnOnce(models::Map, models::Player) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let (map, author) = new_map(db.clone(), true).await?;
    let id = map.id;
    let out = f(map, author).await?;
    remove_map(db, id, true).await?;
    Ok(out)
}
