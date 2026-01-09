use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Records::Table)
                    .add_column(
                        ColumnDef::new(Records::IsHidden)
                            .boolean()
                            .default(false)
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
                    .table(Records::Table)
                    .drop_column(Records::IsHidden)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Records {
    Table,
    IsHidden,
}
