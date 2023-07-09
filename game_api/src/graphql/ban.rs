use async_graphql::{dataloader::DataLoader, Context};

use crate::models::{Banishment, Player};

use super::player::PlayerLoader;

#[async_graphql::Object]
impl Banishment {
    async fn id(&self) -> u32 {
        self.inner.id
    }

    async fn date_ban(&self) -> chrono::NaiveDateTime {
        self.inner.date_ban
    }

    async fn duration(&self) -> i64 {
        self.inner.duration
    }

    async fn was_reprieved(&self) -> bool {
        self.was_reprieved
    }

    async fn reason(&self) -> &str {
        &self.inner.reason
    }

    async fn player(&self, ctx: &Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Bannished player not found."))
    }

    async fn banished_by(&self, ctx: &Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.banished_by)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Bannisher player not found."))
    }
}
