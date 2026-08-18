use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::checkpoint_times;
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, QueryOrder as _,
    QuerySelect as _, prelude::Expr, sea_query::Func,
};

use crate::{error::ApiGqlError, objects::checkpoint_time::CheckpointTime};

/// Loads the average checkpoint times of maps, keyed by map ID.
pub struct MapAverageCpsTimesLoader(pub DbConn);

impl Loader<u32> for MapAverageCpsTimesLoader {
    type Value = Vec<CheckpointTime>;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let times = checkpoint_times::Entity::find()
            .filter(checkpoint_times::Column::MapId.is_in(keys.iter().copied()))
            .group_by(checkpoint_times::Column::MapId)
            .group_by(checkpoint_times::Column::CpNum)
            .order_by_asc(checkpoint_times::Column::CpNum)
            .select_only()
            .columns([
                checkpoint_times::Column::CpNum,
                checkpoint_times::Column::MapId,
                checkpoint_times::Column::RecordId,
            ])
            .expr_as(
                Func::cust("FLOOR").arg(Func::avg(Expr::col(checkpoint_times::Column::Time))),
                "time",
            )
            .into_model::<CheckpointTime>()
            .all(&self.0)
            .await?;

        let mut hashmap: HashMap<_, Vec<_>> = HashMap::new();

        // The rows are already sorted by checkpoint number, so appending them in order keeps
        // each map's checkpoints sorted too.
        for time in times {
            hashmap.entry(time.inner.map_id).or_default().push(time);
        }

        Ok(hashmap)
    }
}
