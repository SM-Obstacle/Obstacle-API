use entity::checkpoint_times;
use sea_orm_migration::prelude::*;

const INDEX_NAME: &str = "idx_checkpoint_times_on_map_cp_num_time";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name(INDEX_NAME)
                    .table(checkpoint_times::Entity)
                    .col(checkpoint_times::Column::MapId)
                    .col(checkpoint_times::Column::CpNum)
                    .col(checkpoint_times::Column::Time)
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(INDEX_NAME)
                    .table(checkpoint_times::Entity)
                    .to_owned(),
            )
            .await
    }
}
