use std::{future::ready, pin::Pin};

use sea_orm::entity::prelude::*;

use crate::banishments;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "current_bans")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: u32,
    pub date_ban: DateTime,
    pub duration: Option<i64>,
    pub was_reprieved: i8,
    pub reason: String,
    pub player_id: Option<u32>,
    pub banished_by: Option<u32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::BanishedBy",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    BanAuthor,
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    Player,
}

impl ActiveModelBehavior for ActiveModel {
    fn before_save<'a, 'b, C>(
        self,
        _db: &'a C,
        _insert: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Self, DbErr>> + Send + 'b>>
    where
        C: ConnectionTrait,
        C: 'b,
        'a: 'b,
        Self: Send + 'b,
    {
        Box::pin(ready(Err(DbErr::RecordNotInserted)))
    }

    fn before_delete<'a, 'b, C>(
        self,
        _db: &'a C,
    ) -> Pin<Box<dyn Future<Output = Result<Self, DbErr>> + Send + 'b>>
    where
        C: ConnectionTrait,
        C: 'b,
        'a: 'b,
        Self: Send + 'b,
    {
        Box::pin(ready(Err(DbErr::RecordNotUpdated)))
    }
}

impl From<Model> for banishments::Model {
    fn from(value: Model) -> Self {
        Self {
            id: value.id,
            date_ban: value.date_ban,
            duration: value.duration,
            was_reprieved: value.was_reprieved,
            reason: value.reason,
            player_id: value.player_id,
            banished_by: value.banished_by,
        }
    }
}
