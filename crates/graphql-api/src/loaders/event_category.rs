use std::{collections::HashMap, sync::Arc};

use async_graphql::dataloader::Loader;
use entity::event_category;
use sea_orm::{ColumnTrait as _, DbConn, DbErr, EntityTrait as _, QueryFilter as _};

pub struct EventCategoryLoader(pub DbConn);

impl Loader<u32> for EventCategoryLoader {
    type Value = event_category::Model;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = event_category::Entity::find()
            .filter(event_category::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|category| (category.id, category))
            .collect();

        Ok(hashmap)
    }
}
