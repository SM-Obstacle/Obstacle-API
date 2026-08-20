use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .add_column(integer_null(Maps::MxId))
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .drop_column(Maps::MxId)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Maps {
    Table,
    MxId,
}
