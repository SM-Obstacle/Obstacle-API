use async_graphql::{dataloader::DataLoader, Context};

use crate::models::{Map, Player, PlayerRating, Rating, RatingKind};

use super::{map::MapLoader, player::PlayerLoader};

#[async_graphql::Object]
impl Rating {
    async fn ratings(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PlayerRating>> {
        let db = ctx.data_unchecked();
        Ok(
            sqlx::query_as("SELECT * FROM player_rating WHERE player_id = ? AND map_id = ?")
                .bind(self.player_id)
                .bind(self.map_id)
                .fetch_all(db)
                .await?,
        )
    }

    async fn player(&self, ctx: &Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<DataLoader<MapLoader>>()
            .load_one(self.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn rating_date(&self) -> chrono::NaiveDateTime {
        self.rating_date
    }
}

#[async_graphql::Object]
impl PlayerRating {
    async fn kind(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<RatingKind> {
        let db = ctx.data_unchecked();
        Ok(sqlx::query_as("SELECT * FROM rating_kind WHERE id = ?")
            .bind(self.kind)
            .fetch_one(db)
            .await?)
    }

    async fn rating(&self) -> f32 {
        self.rating
    }
}
