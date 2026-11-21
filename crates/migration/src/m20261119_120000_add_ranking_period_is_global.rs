use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RankingPeriod::Table)
                    .add_column(
                        ColumnDef::new(RankingPeriod::IsGlobal)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RankingPeriod::Table)
                    .drop_column(RankingPeriod::IsGlobal)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum RankingPeriod {
    Table,
    IsGlobal,
}
