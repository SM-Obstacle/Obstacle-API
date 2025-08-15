use async_graphql::{Context, dataloader::DataLoader};
use entity::{player_rating, rating_kind, types};
use records_lib::models;
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait, FromQueryResult, QueryFilter};

use super::{
    map::{Map, MapLoader},
    player::{Player, PlayerLoader},
};

// TODO: where is this used?
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Rating {
    #[sqlx(flatten)]
    inner: models::Rating,
}

impl From<models::Rating> for Rating {
    fn from(inner: models::Rating) -> Self {
        Self { inner }
    }
}

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
impl Rating {
    async fn ratings(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PlayerRating>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let all = player_rating::Entity::find()
            .filter(
                player_rating::Column::PlayerId
                    .eq(self.inner.player_id)
                    .and(player_rating::Column::MapId.eq(self.inner.map_id)),
            )
            .into_model()
            .all(conn)
            .await?;
        Ok(all)
    }

    async fn player(&self, ctx: &Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<DataLoader<MapLoader>>()
            .load_one(self.inner.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn rating_date(&self) -> chrono::NaiveDateTime {
        self.inner.rating_date
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
