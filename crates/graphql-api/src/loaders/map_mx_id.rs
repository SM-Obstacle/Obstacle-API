use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use records_lib::map;

use crate::error::ApiGqlError;

pub struct MapMxIdLoader(pub reqwest::Client);

impl Loader<String> for MapMxIdLoader {
    type Value = i32;
    type Error = ApiGqlError;

    async fn load(&self, map_uids: &[String]) -> Result<HashMap<String, Self::Value>, Self::Error> {
        map::fetch_mx_map_ids(
            &self.0,
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
