use sea_orm::{
    sqlx::types::chrono::Utc, ActiveModelTrait as _, ActiveValue::Set, ColumnTrait as _,
    EntityTrait, QueryFilter as _,
};
use sea_orm_migration::prelude::*;

use crate::m20250806_191853_first::entities;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        entities::role::Entity::insert_many([
            entities::role::ActiveModel {
                id: Set(1),
                role_name: Set("player".to_owned()),
                privileges: Set(Some(1)),
            },
            entities::role::ActiveModel {
                id: Set(2),
                role_name: Set("mod".to_owned()),
                privileges: Set(Some(3)),
            },
            entities::role::ActiveModel {
                id: Set(3),
                role_name: Set("admin".to_owned()),
                privileges: Set(Some(255)),
            },
        ])
        .exec(manager.get_connection())
        .await?;

        let api_status_normal = entities::api_status::ActiveModel {
            status_id: Set(1),
            status_name: Set("normal".to_owned()),
        };
        let api_status_maintenance = entities::api_status::ActiveModel {
            status_id: Set(2),
            status_name: Set("maintenance".to_owned()),
        };

        entities::api_status::Entity::insert_many([api_status_normal, api_status_maintenance])
            .exec(manager.get_connection())
            .await?;

        entities::api_status_history::ActiveModel {
            status_history_id: Set(1),
            status_id: Set(1),
            status_history_date: Set(Some(Utc::now().naive_utc())),
        }
        .insert(manager.get_connection())
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        entities::role::Entity::delete_many()
            .filter(entities::role::Column::Id.is_in([1, 2, 3]))
            .exec(manager.get_connection())
            .await?;

        entities::api_status_history::Entity::delete_by_id((1, 1))
            .exec(manager.get_connection())
            .await?;

        entities::api_status::Entity::delete_many()
            .filter(entities::api_status::Column::StatusId.is_in([1, 2]))
            .exec(manager.get_connection())
            .await?;

        Ok(())
    }
}
