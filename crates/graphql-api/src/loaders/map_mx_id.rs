use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use mx_layer::maps::CachedMxMapIds;

use crate::error::ApiGqlError;

pub struct MapMxIdLoader(pub CachedMxMapIds);

impl Loader<String> for MapMxIdLoader {
    type Value = i32;
    type Error = ApiGqlError;

    async fn load(&self, map_uids: &[String]) -> Result<HashMap<String, Self::Value>, Self::Error> {
        self.0
            .get_map_ids(
                map_uids
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await
            .map_err(From::from)
    }
}
