use crate::{
    player::{self, UpdatePlayerBody},
    Database, RecordsResult,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct MapAuthor {
    login: String,
    nickname: String,
    zone_path: String,
}

#[derive(Deserialize)]
pub struct UpdateMapBody {
    name: String,
    #[serde(alias = "maniaplanetMapId")]
    pub map_uid: String,
    author: MapAuthor,
}

pub async fn get_or_insert(db: &Database, body: &UpdateMapBody) -> RecordsResult<u32> {
    if let Some(id) = sqlx::query_scalar!("SELECT id FROM maps WHERE game_id = ?", body.map_uid)
        .fetch_optional(&db.mysql_pool)
        .await?
    {
        println!("map exists");
        return Ok(id);
    }

    println!("map doesnt exist, we insert");

    let player_id = player::update_or_insert(
        db,
        &body.author.login,
        UpdatePlayerBody {
            nickname: body.author.nickname.clone(),
            country: body.author.zone_path.clone(),
        },
    )
    .await?;

    let id = sqlx::query_scalar(
        "INSERT INTO maps
        (game_id, player_id, name)
        VALUES (?, ?, ?) RETURNING id",
    )
    .bind(&body.map_uid)
    .bind(player_id)
    .bind(&body.name)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
}
