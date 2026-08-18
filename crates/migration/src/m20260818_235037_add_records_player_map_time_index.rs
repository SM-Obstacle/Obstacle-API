use entity::records;
use sea_orm_migration::prelude::*;

/// Name of the index covering the anti-joins of the `global_records` and `global_event_records`
/// views.
const INDEX_NAME: &str = "idx_records_on_player_map_time";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The `global_records` and `global_event_records` views find the personal best of a player
        // on a map with two anti-joins on `records`: one looking for a strictly better time, the
        // other for a more recent record with the same time.
        //
        // Both match on `(record_player_id, map_id)` then compare `time` and `record_id`. The
        // closest existing index, `idx_records_on_player_map_date`, stops at `record_date`, so
        // every candidate row needed a primary key lookup just to read `time`. Adding `time` and
        // `record_id` makes both anti-joins index-only.
        manager
            .create_index(
                Index::create()
                    .name(INDEX_NAME)
                    .table(records::Entity)
                    .col(records::Column::RecordPlayerId)
                    .col(records::Column::MapId)
                    .col(records::Column::Time)
                    .col(records::Column::RecordId)
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(INDEX_NAME)
                    .table(records::Entity)
                    .to_owned(),
            )
            .await
    }
}
