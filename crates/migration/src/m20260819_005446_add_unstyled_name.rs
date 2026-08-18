use sea_orm_migration::prelude::*;

/// Length of the `name` columns of `players` and `maps`, mirrored by their `unstyled_name`.
///
/// Stripping the ManiaPlanet style codes can only shorten a name, so this is always enough.
const NAME_LEN: u32 = 512;

const IDX_PLAYERS: &str = "idx_players_unstyled_name";
const IDX_MAPS: &str = "idx_maps_unstyled_name";

/// The tables carrying an `unstyled_name`, with the trigger prefix used for each of them.
const TABLES: [&str; 2] = ["players", "maps"];

/// Builds the trigger keeping `unstyled_name` in sync with `name`.
///
/// `unstyled_name` is deliberately *not* a generated column: the expression would have to be
/// inlined in the column definition (generated columns can't call a stored function), duplicating
/// `rm_mp_style` and turning any future change to it into a table rebuild. A trigger keeps
/// `rm_mp_style` as the single definition, and covers every writer -- the game API, the `admin`
/// crate, the test suite and manual SQL alike -- without any of them having to know the column
/// exists.
fn create_trigger(table: &str, timing: &str) -> String {
    format!(
        "create trigger {table}_unstyled_name_before_{timing} before {timing} on {table} \
         for each row set new.unstyled_name = rm_mp_style(new.name)"
    )
}

fn drop_trigger(table: &str, timing: &str) -> String {
    format!("drop trigger {table}_unstyled_name_before_{timing}")
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Added as nullable first, so the existing rows can be backfilled before the column is
        // made mandatory.
        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .add_column(
                        ColumnDef::new(Players::UnstyledName)
                            .string_len(NAME_LEN)
                            .null(),
                    )
                    .take(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .add_column(
                        ColumnDef::new(Maps::UnstyledName)
                            .string_len(NAME_LEN)
                            .null(),
                    )
                    .take(),
            )
            .await?;

        // Backfill through `rm_mp_style`, so the existing rows go through exactly the same
        // definition the triggers below will apply to the new ones.
        for table in TABLES {
            manager
                .get_connection()
                .execute_unprepared(&format!(
                    "update {table} set unstyled_name = rm_mp_style(name)"
                ))
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .modify_column(
                        ColumnDef::new(Players::UnstyledName)
                            .string_len(NAME_LEN)
                            .not_null(),
                    )
                    .take(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .modify_column(
                        ColumnDef::new(Maps::UnstyledName)
                            .string_len(NAME_LEN)
                            .not_null(),
                    )
                    .take(),
            )
            .await?;

        // Searching and sorting on the unstyled name is the point of the column, and both need the
        // index to avoid reading the table. The primary key is appended so a keyset pagination
        // ordered by name stays inside the index.
        manager
            .create_index(
                Index::create()
                    .name(IDX_PLAYERS)
                    .table(Players::Table)
                    .col(Players::UnstyledName)
                    .col(Players::Id)
                    .take(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name(IDX_MAPS)
                    .table(Maps::Table)
                    .col(Maps::UnstyledName)
                    .col(Maps::Id)
                    .take(),
            )
            .await?;

        for table in TABLES {
            for timing in ["insert", "update"] {
                manager
                    .get_connection()
                    .execute_unprepared(&create_trigger(table, timing))
                    .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in TABLES {
            for timing in ["insert", "update"] {
                manager
                    .get_connection()
                    .execute_unprepared(&drop_trigger(table, timing))
                    .await?;
            }
        }

        manager
            .drop_index(
                Index::drop()
                    .name(IDX_PLAYERS)
                    .table(Players::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(Index::drop().name(IDX_MAPS).table(Maps::Table).to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Players::Table)
                    .drop_column(Players::UnstyledName)
                    .take(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Maps::Table)
                    .drop_column(Maps::UnstyledName)
                    .take(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Players {
    Table,
    Id,
    UnstyledName,
}

#[derive(DeriveIden)]
enum Maps {
    Table,
    Id,
    UnstyledName,
}
