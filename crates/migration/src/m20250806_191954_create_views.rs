mod view_creation;

use sea_orm::{EntityTrait as _, PaginatorTrait as _};
use sea_orm_migration::prelude::*;

use crate::m20250806_191853_first::entities;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(view_creation::CREATE_VIEW_CURRENT_BANS)
            .await?;
        assert_eq!(
            entities::current_bans::Entity::find()
                .count(manager.get_connection())
                .await?,
            0
        );

        manager
            .get_connection()
            .execute_unprepared(view_creation::CREATE_VIEW_GLOBAL_RECORDS)
            .await?;
        assert_eq!(
            entities::global_records::Entity::find()
                .count(manager.get_connection())
                .await?,
            0
        );

        manager
            .get_connection()
            .execute_unprepared(view_creation::CREATE_VIEW_GLOBAL_EVENT_RECORDS)
            .await?;
        assert_eq!(
            entities::global_event_records::Entity::find()
                .count(manager.get_connection())
                .await?,
            0
        );

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: "drop view global_event_records".to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
            .await?;

        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: "drop view global_records".to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
            .await?;

        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: "drop view current_bans".to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
            .await?;

        Ok(())
    }
}
