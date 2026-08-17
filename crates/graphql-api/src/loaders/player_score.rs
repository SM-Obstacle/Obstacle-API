use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::player_periodic_ranking;
use sea_orm::{ColumnTrait, DbConn, EntityTrait as _, QueryFilter as _, QuerySelect};

use crate::{error::ApiGqlError, utils::get_last_period_id};

pub struct PlayerScoreLoader(pub DbConn);

impl Loader<u32> for PlayerScoreLoader {
    type Value = f64;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let Some(last_period_id) = get_last_period_id(&self.0).await else {
            return Ok(Default::default());
        };

        let hashmap = player_periodic_ranking::Entity::find()
            .select_only()
            .columns([
                player_periodic_ranking::Column::PlayerId,
                player_periodic_ranking::Column::Score,
            ])
            .filter(
                player_periodic_ranking::Column::PlayerId
                    .is_in(keys.iter().copied())
                    .and(player_periodic_ranking::Column::PeriodId.eq(last_period_id)),
            )
            .into_tuple()
            .all(&self.0)
            .await?
            .into_iter()
            .collect();

        Ok(hashmap)
    }
}
