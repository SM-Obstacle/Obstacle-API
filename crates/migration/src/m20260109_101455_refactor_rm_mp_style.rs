mod func_management;

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("drop function rm_mp_style")
            .await?;
        manager
            .get_connection()
            .execute_unprepared(func_management::CREATE_FUNCTION_UNSTYLED)
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("drop function unstyled")
            .await?;
        manager
            .get_connection()
            .execute_unprepared(func_management::CREATE_FUNCTION_RM_MP_STYLE)
            .await?;
        Ok(())
    }
}
