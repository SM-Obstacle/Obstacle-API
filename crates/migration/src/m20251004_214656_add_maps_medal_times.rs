use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .add_column(ColumnDef::new(Maps::BronzeTime).null().integer().take())
                    .add_column(ColumnDef::new(Maps::SilverTime).null().integer().take())
                    .add_column(ColumnDef::new(Maps::GoldTime).null().integer().take())
                    .add_column(ColumnDef::new(Maps::AuthorTime).null().integer().take())
                    .take(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .drop_column(Maps::BronzeTime)
                    .drop_column(Maps::SilverTime)
                    .drop_column(Maps::GoldTime)
                    .drop_column(Maps::AuthorTime)
                    .take(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Maps {
    Table,
    BronzeTime,
    SilverTime,
    GoldTime,
    AuthorTime,
}
