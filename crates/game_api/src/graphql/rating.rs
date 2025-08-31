use entity::{player_rating, rating_kind, types};
use sea_orm::{DbConn, EntityTrait, FromQueryResult};

#[derive(Debug, Clone, FromQueryResult)]
pub struct PlayerRating {
    #[sea_orm(nested)]
    inner: player_rating::Model,
}

impl From<player_rating::Model> for PlayerRating {
    fn from(inner: player_rating::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl PlayerRating {
    async fn kind(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<types::RatingKind> {
        let conn = ctx.data_unchecked::<DbConn>();

        let kind = rating_kind::Entity::find_by_id(self.inner.kind)
            .into_model()
            .one(conn)
            .await?
            .unwrap_or_else(|| {
                panic!(
                    "Rating kind with ID {} must exist in database",
                    self.inner.kind
                )
            });

        Ok(kind)
    }

    async fn rating(&self) -> f32 {
        self.inner.rating
    }
}
