use std::collections::HashMap;

use async_graphql::dataloader::Loader;
use entity::records;
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, QuerySelect as _, prelude::Expr,
    sea_query::Func,
};

use crate::error::ApiGqlError;

/// Loads the total amount of tries a player spent on a map, keyed by `(player_id, map_id)`.
pub struct TryCountLoader(pub DbConn);

impl Loader<(u32, u32)> for TryCountLoader {
    type Value = i32;
    type Error = ApiGqlError;

    async fn load(
        &self,
        keys: &[(u32, u32)],
    ) -> Result<HashMap<(u32, u32), Self::Value>, Self::Error> {
        // MariaDB turns a tuple `IN` into an index range, unlike a tuple comparison, so this
        // uses the `(record_player_id, map_id, record_date)` index.
        let pairs = Expr::tuple([
            Expr::col(records::Column::RecordPlayerId).into(),
            Expr::col(records::Column::MapId).into(),
        ])
        .in_tuples(keys.iter().copied());

        let rows: Vec<(u32, u32, Option<i32>)> = records::Entity::find()
            .filter(pairs)
            .select_only()
            .column(records::Column::RecordPlayerId)
            .column(records::Column::MapId)
            .expr(Func::cast_as(records::Column::TryCount.sum(), "INT"))
            .group_by(records::Column::RecordPlayerId)
            .group_by(records::Column::MapId)
            .into_tuple()
            .all(&self.0)
            .await?;

        let hashmap = rows
            .into_iter()
            // The try count is nullable, and stays so once summed over records that never
            // recorded one. Such a player still played the map at least once.
            .map(|(player_id, map_id, sum)| ((player_id, map_id), sum.unwrap_or(1)))
            .collect();

        Ok(hashmap)
    }
}
