use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::maps;
use mx_layer::maps::CachedMxMapIds;
use records_lib::sync;
use sea_orm::{
    ActiveValue::Set, ColumnTrait as _, ConnectionTrait, DbConn, DbErr, EntityTrait as _,
    QueryFilter as _, QuerySelect,
};

use crate::{
    error::{ApiGqlError, GqlResult},
    objects::map::Map,
};

pub struct MapLoader(pub DbConn, pub CachedMxMapIds);

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
        let maps_without_mx_id = maps::Entity::find()
            .filter(
                maps::Column::MxId
                    .is_null()
                    .and(maps::Column::Id.is_in(keys.iter().copied())),
            )
            .select_only()
            .columns([maps::Column::Id, maps::Column::GameId])
            .into_tuple::<(u32, String)>()
            .all(&self.0)
            .await?;

        let map_uids_without_mx_id = maps_without_mx_id
            .iter()
            .map(|(_, map_uid)| map_uid.as_str())
            .collect::<Vec<_>>();

        if map_uids_without_mx_id.is_empty() {
            return load_maps(&self.0, keys).await;
        }

        let mx_id_map = self.1.get_map_ids(&map_uids_without_mx_id).await?;
        let mut maps_with_mx_id = maps_without_mx_id
            .into_iter()
            .filter_map(|(map_id, map_uid)| {
                mx_id_map.get(&map_uid).map(|mx_id| maps::ActiveModel {
                    id: Set(map_id),
                    mx_id: Set(Some(*mx_id)),
                    ..Default::default()
                })
            })
            .peekable();

        // We don't really bother ourselves here in order to do an SQL batch update...
        // we just do a transaction for each chunk
        while maps_with_mx_id.peek().is_some() {
            sync::transaction(&self.0, async |txn| {
                for _ in 0..100 {
                    let Some(update) = maps_with_mx_id.next() else {
                        break;
                    };
                    maps::Entity::update(update).exec(txn).await?;
                }
                Ok::<_, DbErr>(())
            })
            .await?;
        }

        load_maps(&self.0, keys).await
    }
}
