use std::{collections::HashMap, sync::Arc};

use async_graphql::dataloader::Loader;
use entity::players;
use sea_orm::{ColumnTrait as _, DbConn, DbErr, EntityTrait as _, QueryFilter as _};

use crate::objects::player::Player;

pub struct PlayerLoader(pub DbConn);

impl Loader<u32> for PlayerLoader {
    type Value = Player;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = players::Entity::find()
            .filter(players::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|player| (player.id, player.into()))
            .collect();

        Ok(hashmap)
    }
}
