use core::fmt;

use sqlx::{Executor, MySql};

use crate::{error::RecordsResult, models::Map};

pub async fn get_map_from_game_id<'c, E: Executor<'c, Database = MySql>>(
    db: E,
    map_game_id: &str,
) -> RecordsResult<Option<Map>> {
    let r = sqlx::query_as("SELECT * FROM maps WHERE game_id = ?")
        .bind(map_game_id)
        .fetch_optional(db)
        .await?;
    Ok(r)
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
pub struct MxMappackMapItem {
    pub TrackUID: String,
    pub MapID: i64,
    pub GbxMapName: String,
    pub AuthorLogin: String,
}

pub async fn fetch_mx_mappack_maps(
    client: &reqwest::Client,
    mappack_id: u32,
    secret: Option<&str>,
) -> RecordsResult<Vec<MxMappackMapItem>> {
    struct SecretParam<'a>(Option<&'a str>);

    impl fmt::Display for SecretParam<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(s) = self.0 {
                write!(f, "?secret={s}")?;
            }
            Ok(())
        }
    }

    let secret = SecretParam(secret);

    client
        .get(format!(
            "https://sm.mania.exchange/api/mappack/get_mappack_tracks/{mappack_id}{secret}"
        ))
        .header("User-Agent", "obstacle (discord @ahmadbky)")
        .send()
        .await?
        .json()
        .await
        .map_err(From::from)
}
