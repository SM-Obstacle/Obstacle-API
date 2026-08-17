use entity::{maps, players};
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create the period table
        manager
            .create_table(
                Table::create()
                    .table(RankingPeriod::Table)
                    .col(pk_auto(RankingPeriod::PeriodId))
                    .col(date_time(RankingPeriod::PeriodDate))
                    .take(),
            )
            .await?;

        // Create player periodic ranking table
        manager
            .create_table(
                Table::create()
                    .table(PlayerPeriodicRanking::Table)
                    .col(double(PlayerPeriodicRanking::Score))
                    .col(integer(PlayerPeriodicRanking::PeriodId))
                    .col(unsigned(PlayerPeriodicRanking::PlayerId))
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                PlayerPeriodicRanking::Table,
                                PlayerPeriodicRanking::PlayerId,
                            )
                            .to(Players::Table, players::Column::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                PlayerPeriodicRanking::Table,
                                PlayerPeriodicRanking::PeriodId,
                            )
                            .to(RankingPeriod::Table, RankingPeriod::PeriodId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .primary_key(
                        Index::create()
                            .col(PlayerPeriodicRanking::PlayerId)
                            .col(PlayerPeriodicRanking::PeriodId),
                    )
                    .take(),
            )
            .await?;

        // Create map periodic ranking table
        manager
            .create_table(
                Table::create()
                    .table(MapPeriodicRanking::Table)
                    .col(double(MapPeriodicRanking::Score))
                    .col(integer(MapPeriodicRanking::PeriodId))
                    .col(unsigned(MapPeriodicRanking::MapId))
                    .foreign_key(
                        ForeignKey::create()
                            .from(MapPeriodicRanking::Table, MapPeriodicRanking::MapId)
                            .to(Maps::Table, maps::Column::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MapPeriodicRanking::Table, MapPeriodicRanking::PeriodId)
                            .to(RankingPeriod::Table, RankingPeriod::PeriodId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .primary_key(
                        Index::create()
                            .col(MapPeriodicRanking::MapId)
                            .col(MapPeriodicRanking::PeriodId),
                    )
                    .take(),
            )
            .await?;

        // Remove "score" column from players and maps table
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add "score" column to players and maps table
        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .add_column(double(Players::Score).default(0))
                    .take(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .add_column(double(Maps::Score).default(0))
                    .take(),
            )
            .await?;

        // Remove tables "map periodic ranking", "player periodic ranking" then "period" table.
        manager
            .drop_table(Table::drop().table(PlayerPeriodicRanking::Table).take())
            .await?;
        manager
            .drop_table(Table::drop().table(MapPeriodicRanking::Table).take())
            .await?;
        manager
            .drop_table(Table::drop().table(RankingPeriod::Table).cascade().take())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum RankingPeriod {
    Table,
    PeriodId,
    PeriodDate,
}

#[derive(DeriveIden)]
enum PlayerPeriodicRanking {
    Table,
    PlayerId,
    PeriodId,
    Score,
}

#[derive(DeriveIden)]
enum MapPeriodicRanking {
    Table,
    MapId,
    PeriodId,
    Score,
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
