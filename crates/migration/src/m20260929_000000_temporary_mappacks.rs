use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Mappacks::Table)
                    .add_column(ColumnDef::new(Mappacks::ExpiresAt).date_time())
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Mappacks::Table)
                    .drop_column(Mappacks::ExpiresAt)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Mappacks {
    Table,
    ExpiresAt,
}
