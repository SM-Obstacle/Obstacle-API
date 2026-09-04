//! Where the MX IDs fetched by the [`mx_layer`] end up in our database.

use std::collections::HashMap;

use entity::maps;
use mx_layer::maps::MxIdSink;
use records_lib::error::RecordsResult;
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, prelude::Expr};

/// Saves the MX IDs in the `mx_id` column of the maps they belong to.
///
/// Nothing waits for this: it's called once the MX API answered, and once whoever asked for these
/// map UIDs got them.
pub struct DbMxIdSink(DbConn);

impl DbMxIdSink {
    #[inline]
    pub fn new(conn: DbConn) -> Self {
        Self(conn)
    }
}

impl MxIdSink for DbMxIdSink {
    #[allow(clippy::manual_async_fn)]
    fn store<'a>(
        &'a self,
        map_ids: &'a HashMap<String, i32>,
    ) -> impl Future<Output = RecordsResult> + Send + 'a {
        async move {
            // A batch holds at most a few dozen maps, and they're each a different value to set,
            // so a statement per map is fine here.
            for (map_uid, mx_id) in map_ids {
                maps::Entity::update_many()
                    .col_expr(maps::Column::MxId, Expr::value(*mx_id))
                    .filter(
                        maps::Column::GameId
                            .eq(map_uid)
                            .and(maps::Column::MxId.is_null()),
                    )
                    .exec(&self.0)
                    .await?;
            }

            Ok(())
        }
    }
}
