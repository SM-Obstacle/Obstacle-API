use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::event_edition_maps;
use sea_orm::{DbConn, EntityTrait as _, QueryFilter as _, prelude::Expr};

use crate::error::ApiGqlError;

/// The key of an [`EventEditionMapLoader`] entry: `(event_id, edition_id, map_id)`.
pub type EventEditionMapKey = (u32, u32, u32);

/// Loads the rows binding maps to event editions.
///
/// Several fields of an event edition map are read off the same row, so they all share this
/// loader instead of querying it once each.
pub struct EventEditionMapLoader(pub DbConn);

impl Loader<EventEditionMapKey> for EventEditionMapLoader {
    type Value = event_edition_maps::Model;
    type Error = ApiGqlError;

    async fn load(
        &self,
        keys: &[EventEditionMapKey],
    ) -> Result<HashMap<EventEditionMapKey, Self::Value>, Self::Error> {
        // MariaDB turns a tuple `IN` into an index range, unlike a tuple comparison, so this
        // uses the primary key.
        let ids = Expr::tuple([
            Expr::col(event_edition_maps::Column::EventId).into(),
            Expr::col(event_edition_maps::Column::EditionId).into(),
            Expr::col(event_edition_maps::Column::MapId).into(),
        ])
        .in_tuples(keys.iter().copied());

        let hashmap = event_edition_maps::Entity::find()
            .filter(ids)
            .all(&self.0)
            .await?
            .into_iter()
            .map(|row| ((row.event_id, row.edition_id, row.map_id), row))
            .collect();

        Ok(hashmap)
    }
}
