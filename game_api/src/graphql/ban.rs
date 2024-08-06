use async_graphql::{dataloader::DataLoader, Context};

use records_lib::models;

use super::player::{Player, PlayerLoader};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Banishment {
    #[sqlx(flatten)]
    inner: models::Banishment,
}

impl From<models::Banishment> for Banishment {
    fn from(inner: models::Banishment) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Banishment {
    async fn id(&self) -> u32 {
        self.inner.id
    }

    async fn date_ban(&self) -> chrono::NaiveDateTime {
        self.inner.date_ban
    }

    async fn duration(&self) -> Option<i64> {
        self.inner.duration
    }

    async fn was_reprieved(&self) -> bool {
        self.inner.was_reprieved
    }

    async fn reason(&self) -> &str {
        &self.inner.reason
    }

    async fn player(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Player>> {
        match self.inner.player_id {
            Some(player_id) => match ctx
                .data_unchecked::<DataLoader<PlayerLoader>>()
                .load_one(player_id)
                .await?
            {
                Some(player) => Ok(Some(player)),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }

    async fn banished_by(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Player>> {
        match self.inner.banished_by {
            Some(banished_by) => match ctx
                .data_unchecked::<DataLoader<PlayerLoader>>()
                .load_one(banished_by)
                .await?
            {
                Some(player) => Ok(Some(player)),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }
}
