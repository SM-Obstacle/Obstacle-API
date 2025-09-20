use std::{collections::HashMap, sync::Arc};

use async_graphql::dataloader::Loader;
use entity::event;
use sea_orm::{ColumnTrait as _, DbConn, DbErr, EntityTrait as _, QueryFilter as _};

use crate::objects::event::Event;

pub struct EventLoader(pub DbConn);

impl Loader<u32> for EventLoader {
    type Value = Event;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = event::Entity::find()
            .filter(event::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|row| (row.id, row.into()))
            .collect();
        Ok(hashmap)
    }
}
