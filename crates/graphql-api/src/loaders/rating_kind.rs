use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::{rating_kind, types};
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _};

use crate::{error::ApiGqlError, objects::rating_kind::RatingKind};

/// Loads rating kinds by ID.
pub struct RatingKindLoader(pub DbConn);

impl Loader<u8> for RatingKindLoader {
    type Value = RatingKind;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u8]) -> Result<HashMap<u8, Self::Value>, Self::Error> {
        let kinds = rating_kind::Entity::find()
            .filter(rating_kind::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?;

        let hashmap = kinds
            .into_iter()
            .map(|kind| Ok((kind.id, types::RatingKind::try_from(&kind)?.into())))
            .collect::<Result<_, sea_orm::DbErr>>()?;

        Ok(hashmap)
    }
}
