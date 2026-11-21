use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use mx_layer::maps::MxIdProvider;

use crate::error::ApiGqlError;

pub struct MapMxIdLoader(pub MxIdProvider);

impl Loader<String> for MapMxIdLoader {
    type Value = i32;
    type Error = ApiGqlError;

    async fn load(&self, map_uids: &[String]) -> Result<HashMap<String, Self::Value>, Self::Error> {
        // This waits for at most one request to MX, and never for its batching window: a map UID
        // which has to wait for it is missing from the result, and fetched in the background for
        // the next queries.
        Ok(self
            .0
            .get_mx_ids_of_map_uids(
                map_uids
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .await)
    }
}
