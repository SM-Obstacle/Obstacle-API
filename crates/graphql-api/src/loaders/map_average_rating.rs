use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::player_rating;
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, QueryOrder as _,
    QuerySelect as _,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::{error::ApiGqlError, objects::player_rating::PlayerRating};

/// Loads the average rating of maps, one entry per rating kind, keyed by map ID.
pub struct MapAverageRatingLoader(pub DbConn);

impl Loader<u32> for MapAverageRatingLoader {
    type Value = Vec<PlayerRating>;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let ratings = player_rating::Entity::find()
            .filter(player_rating::Column::MapId.is_in(keys.iter().copied()))
            .group_by(player_rating::Column::MapId)
            .group_by(player_rating::Column::Kind)
            .order_by_asc(player_rating::Column::Kind)
            .select_only()
            // The average isn't tied to any player, but the model needs the column.
            .expr_as(1.cast_as("UNSIGNED"), "player_id")
            .columns([player_rating::Column::MapId, player_rating::Column::Kind])
            .expr_as(
                Func::avg(Expr::col(player_rating::Column::Rating)),
                "rating",
            )
            .into_model::<PlayerRating>()
            .all(&self.0)
            .await?;

        let mut hashmap: HashMap<_, Vec<_>> = HashMap::new();

        for rating in ratings {
            hashmap.entry(rating.inner.map_id).or_default().push(rating);
        }

        Ok(hashmap)
    }
}
