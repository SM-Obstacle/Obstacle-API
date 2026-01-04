use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .add_column(ColumnDef::new(Players::Score).double().default(0).take())
                    .take(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .add_column(ColumnDef::new(Maps::Score).double().default(0).take())
                    .take(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .drop_column(Players::Score)
                    .take(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .drop_column(Maps::Score)
                    .take(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Players {
    Table,
    Score,
}

#[derive(DeriveIden)]
enum Maps {
    Table,
    Score,
}
