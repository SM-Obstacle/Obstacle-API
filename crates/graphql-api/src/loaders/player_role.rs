use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::role;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter as _};

use crate::error::ApiGqlError;

pub struct PlayerRoleLoader(pub DbConn);

impl Loader<u8> for PlayerRoleLoader {
    type Value = role::Model;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u8]) -> Result<HashMap<u8, Self::Value>, Self::Error> {
        role::Entity::find()
            .filter(role::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await
            .map(|roles| roles.into_iter().map(|role| (role.id, role)).collect())
            .map_err(From::from)
    }
}
