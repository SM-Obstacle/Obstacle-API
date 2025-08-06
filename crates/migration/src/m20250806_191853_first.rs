pub(crate) mod entities;
mod view_creation;

use sea_orm::{EntityTrait, PaginatorTrait as _};
use sea_orm_migration::{prelude::*, sea_orm::Schema};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_database_backend();
        let schema = Schema::new(db);

        create_entity_table(manager, &schema, entities::rating_kind::Entity).await?;
        create_entity_table(manager, &schema, entities::latestnews_image::Entity).await?;
        create_entity_table(manager, &schema, entities::role::Entity).await?;
        create_entity_table(manager, &schema, entities::api_status::Entity).await?;
        create_entity_table(manager, &schema, entities::api_status_history::Entity).await?;

        create_entity_table(manager, &schema, entities::players::Entity).await?;
        create_entity_table(manager, &schema, entities::players_ips::Entity).await?;
        create_entity_table(manager, &schema, entities::banishments::Entity).await?;
        create_entity_table(manager, &schema, entities::maps::Entity).await?;
        create_entity_table(manager, &schema, entities::rating::Entity).await?;
        create_entity_table(manager, &schema, entities::player_rating::Entity).await?;
        create_entity_table(manager, &schema, entities::records::Entity).await?;
        create_entity_table(manager, &schema, entities::checkpoint_times::Entity).await?;

        create_entity_table(manager, &schema, entities::event::Entity).await?;
        create_entity_table(manager, &schema, entities::event_admins::Entity).await?;
        create_entity_table(manager, &schema, entities::event_category::Entity).await?;
        create_entity_table(manager, &schema, entities::event_categories::Entity).await?;
        create_entity_table(
            manager,
            &schema,
            entities::in_game_event_edition_params::Entity,
        )
        .await?;
        create_entity_table(manager, &schema, entities::event_edition::Entity).await?;
        create_entity_table(manager, &schema, entities::event_edition_admins::Entity).await?;
        create_entity_table(manager, &schema, entities::event_edition_categories::Entity).await?;
        create_entity_table(manager, &schema, entities::event_edition_maps::Entity).await?;
        create_entity_table(manager, &schema, entities::event_edition_records::Entity).await?;

        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: view_creation::CREATE_VIEW_CURRENT_BANS.to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
            .await?;
        assert_eq!(
            entities::current_bans::Entity::find()
                .count(manager.get_connection())
                .await?,
            0
        );

        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: view_creation::CREATE_VIEW_GLOBAL_RECORDS.to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
            .await?;
        assert_eq!(
            entities::global_records::Entity::find()
                .count(manager.get_connection())
                .await?,
            0
        );

        manager
            .get_connection()
            .execute(sea_orm::Statement {
                sql: view_creation::CREATE_VIEW_GLOBAL_EVENT_RECORDS.to_owned(),
                values: None,
                db_backend: manager.get_database_backend(),
            })
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
        let db = manager.get_database_backend();
        let schema = Schema::new(db);

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

        drop_entity_table(manager, &schema, entities::event_edition_records::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_edition_maps::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_edition_categories::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_edition_admins::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_edition::Entity).await?;
        drop_entity_table(
            manager,
            &schema,
            entities::in_game_event_edition_params::Entity,
        )
        .await?;
        drop_entity_table(manager, &schema, entities::event_categories::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_category::Entity).await?;
        drop_entity_table(manager, &schema, entities::event_admins::Entity).await?;
        drop_entity_table(manager, &schema, entities::event::Entity).await?;

        drop_entity_table(manager, &schema, entities::checkpoint_times::Entity).await?;
        drop_entity_table(manager, &schema, entities::records::Entity).await?;
        drop_entity_table(manager, &schema, entities::player_rating::Entity).await?;
        drop_entity_table(manager, &schema, entities::rating::Entity).await?;
        drop_entity_table(manager, &schema, entities::maps::Entity).await?;
        drop_entity_table(manager, &schema, entities::banishments::Entity).await?;
        drop_entity_table(manager, &schema, entities::players_ips::Entity).await?;
        drop_entity_table(manager, &schema, entities::players::Entity).await?;

        drop_entity_table(manager, &schema, entities::api_status_history::Entity).await?;
        drop_entity_table(manager, &schema, entities::api_status::Entity).await?;
        drop_entity_table(manager, &schema, entities::role::Entity).await?;
        drop_entity_table(manager, &schema, entities::latestnews_image::Entity).await?;
        drop_entity_table(manager, &schema, entities::rating_kind::Entity).await?;

        Ok(())
    }
}

async fn create_entity_table<'a, E: EntityTrait>(
    manager: &'a SchemaManager<'a>,
    schema: &Schema,
    entity: E,
) -> Result<(), DbErr> {
    manager
        .create_table(schema.create_table_from_entity(entity))
        .await
}

async fn drop_entity_table<'a, E: EntityTrait>(
    manager: &'a SchemaManager<'a>,
    schema: &Schema,
    entity: E,
) -> Result<(), DbErr> {
    manager
        .drop_table(
            Table::drop()
                .table(
                    schema
                        .create_table_from_entity(entity)
                        .get_table_name()
                        .cloned()
                        .unwrap(),
                )
                .take(),
        )
        .await
}
