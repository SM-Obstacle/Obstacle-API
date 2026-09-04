use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::maps;
use mx_layer::maps::MxIdProvider;
use sea_orm::{ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, QueryFilter as _};

use crate::{
    error::{ApiGqlError, GqlResult},
    objects::map::Map,
};

pub struct MapLoader(pub DbConn, pub MxIdProvider);

async fn load_maps<C: ConnectionTrait>(conn: &C, keys: &[u32]) -> GqlResult<HashMap<u32, Map>> {
    let hashmap = maps::Entity::find()
        .filter(maps::Column::Id.is_in(keys.iter().copied()))
        .all(conn)
        .await?
        .into_iter()
        .map(|map| (map.id, map.into()))
        .collect();

    Ok(hashmap)
}

impl Loader<u32> for MapLoader {
    type Value = Map;
    type Error = ApiGqlError;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let mut maps = load_maps(&self.0, keys).await?;

        // Loading a list of maps is a good opportunity to ask for the MX ID of those we don't have
        // yet: the website usually shows a list of maps, then the details of one of them, which
        // includes its MX ID. Asking for them here means they're already fetched by the time
        // somebody actually asks for one.
        //
        // This waits for at most one request to MX, and never for its batching window. Saving what
        // it brings back in our database is the provider's job, not ours.
        let map_uids_without_mx_id = maps
            .values()
            .filter(|map| map.inner.mx_id.is_none())
            .map(|map| map.inner.game_id.as_str())
            .collect::<Vec<_>>();

        if map_uids_without_mx_id.is_empty() {
            return Ok(maps);
        }

        let mx_ids = self.1.get_mx_ids_of_map_uids(&map_uids_without_mx_id).await;

        // The maps we return must hold them too, otherwise the `mxId` field would ask for them
        // again for nothing.
        for map in maps.values_mut() {
            if let Some(&mx_id) = mx_ids.get(&map.inner.game_id) {
                map.inner.mx_id = Some(mx_id);
            }
        }

        Ok(maps)
    }
}
