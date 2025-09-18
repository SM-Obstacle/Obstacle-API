use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(EventEditionMaps::Table)
                    .add_column(
                        ColumnDef::new(EventEditionMaps::Source)
                            .text()
                            .null()
                            .take(),
                    )
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(EventEditionMaps::Table)
                    .drop_column(EventEditionMaps::Source)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum EventEditionMaps {
    Table,
    Source,
}
