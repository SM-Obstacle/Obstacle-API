use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::checkpoint_times;
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, QueryOrder as _};

use crate::{error::ApiGqlError, objects::checkpoint_time::CheckpointTime};

/// Loads the checkpoint times of records, keyed by `(record_id, map_id)`.
pub struct CheckpointTimesLoader(pub DbConn);

impl Loader<(u32, u32)> for CheckpointTimesLoader {
    type Value = Vec<CheckpointTime>;
    type Error = ApiGqlError;

    async fn load(
        &self,
        keys: &[(u32, u32)],
    ) -> Result<HashMap<(u32, u32), Self::Value>, Self::Error> {
        // A record belongs to a single map, so filtering on the record IDs alone selects the
        // same rows as filtering on both, while letting MariaDB use the `record_id` index.
        let times = checkpoint_times::Entity::find()
            .filter(
                checkpoint_times::Column::RecordId
                    .is_in(keys.iter().map(|(record_id, _)| *record_id)),
            )
            .order_by_asc(checkpoint_times::Column::CpNum)
            .all(&self.0)
            .await?;

        let mut hashmap: HashMap<_, Vec<_>> = HashMap::new();

        // The rows are already sorted by checkpoint number, so appending them in order keeps
        // each record's checkpoints sorted too.
        for time in times {
            hashmap
                .entry((time.record_id, time.map_id))
                .or_default()
                .push(time.into());
        }

        Ok(hashmap)
    }
}
