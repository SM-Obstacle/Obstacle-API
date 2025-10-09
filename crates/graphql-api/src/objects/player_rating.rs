use entity::{player_rating, rating_kind, types};
use records_lib::internal;
use sea_orm::{DbConn, EntityTrait as _, FromQueryResult};

use crate::{error::GqlResult, objects::rating_kind::RatingKind};

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
        let conn = ctx.data_unchecked::<DbConn>();

        let kind = rating_kind::Entity::find_by_id(self.inner.kind)
            .into_model::<types::RatingKind>()
            .one(conn)
            .await?
            .ok_or_else(|| {
                internal!(
                    "Rating kind with ID {} must exist in database",
                    self.inner.kind
                )
            })?;

        Ok(kind.into())
    }

    async fn rating(&self) -> f32 {
        self.inner.rating
    }
}
