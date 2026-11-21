use async_graphql::dataloader::DataLoader;
use entity::player_rating;
use records_lib::internal;
use sea_orm::FromQueryResult;

use crate::{
    error::GqlResult, loaders::rating_kind::RatingKindLoader, objects::rating_kind::RatingKind,
};

#[derive(Debug, Clone, FromQueryResult)]
pub struct PlayerRating {
    #[sea_orm(nested)]
    pub inner: player_rating::Model,
}

impl From<player_rating::Model> for PlayerRating {
    fn from(inner: player_rating::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl PlayerRating {
    async fn kind(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<RatingKind> {
        let kind = ctx
            .data_unchecked::<DataLoader<RatingKindLoader>>()
            .load_one(self.inner.kind)
            .await?
            .ok_or_else(|| {
                internal!(
                    "Rating kind with ID {} must exist in database",
                    self.inner.kind
                )
            })?;

        Ok(kind)
    }

    async fn rating(&self) -> f32 {
        self.inner.rating
    }
}
