//! Where the MX IDs fetched by the [`mx_layer`] end up in our database.

use std::collections::HashMap;

use entity::maps;
use mx_layer::maps::MxIdSink;
use records_lib::error::RecordsResult;
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, sea_query::CaseStatement,
};

/// How many maps one `UPDATE` covers at most.
///
/// A batch holds everything asked for during a flush interval, so it may be far bigger than what a
/// single statement should carry.
const CHUNK_SIZE: usize = 256;

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
            let map_ids = map_ids.iter().collect::<Vec<_>>();

            // One statement per chunk: a `CASE` gives each map UID its own value, and the `WHERE`
            // keeps the update to the maps of this chunk which don't have an MX ID yet. Every row
            // the filter lets through matches one of the `WHEN`, so the `CASE` never falls through.
            for chunk in map_ids.chunks(CHUNK_SIZE) {
                let mx_id = chunk
                    .iter()
                    .fold(CaseStatement::new(), |case, (map_uid, mx_id)| {
                        case.case(maps::Column::GameId.eq(map_uid.as_str()), **mx_id)
                    });

                maps::Entity::update_many()
                    .col_expr(maps::Column::MxId, mx_id.into())
                    .filter(
                        maps::Column::GameId
                            .is_in(chunk.iter().map(|(map_uid, _)| map_uid.as_str()))
                            .and(maps::Column::MxId.is_null()),
                    )
                    .exec(&self.0)
                    .await?;
            }

            Ok(())
        }
    }
}
