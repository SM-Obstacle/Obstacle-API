use std::{collections::HashMap, sync::Arc};

use async_graphql::dataloader::Loader;
use entity::maps;
use sea_orm::{ColumnTrait as _, DbConn, DbErr, EntityTrait as _, QueryFilter as _};

use crate::objects::map::Map;

pub struct MapLoader(pub DbConn);

impl Loader<u32> for MapLoader {
    type Value = Map;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = maps::Entity::find()
            .filter(maps::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|map| (map.id, map.into()))
            .collect();

        Ok(hashmap)
    }
}
