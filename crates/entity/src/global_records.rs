use std::{future::ready, pin::Pin};

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "global_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub record_id: u32,
    pub record_player_id: u32,
    pub map_id: u32,
    pub time: i32,
    pub respawn_count: i32,
    pub record_date: DateTime,
    pub flags: u32,
    pub try_count: Option<u32>,
    pub event_record_id: Option<u32>,
    pub modeversion: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::checkpoint_times::Entity")]
    CheckpointTimes,
    #[sea_orm(has_one = "super::event_edition_records::Entity")]
    EventEditionRecords,
    #[sea_orm(
        belongs_to = "super::maps::Entity",
        from = "Column::MapId",
        to = "super::maps::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Maps,
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::RecordPlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "Restrict"
    )]
    Players,
    #[sea_orm(
        belongs_to = "Entity",
        from = "Column::EventRecordId",
        to = "Column::RecordId",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    SelfRef,
}

impl Related<super::checkpoint_times::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CheckpointTimes.def()
    }
}

impl Related<Entity> for super::checkpoint_times::Entity {
    fn to() -> RelationDef {
        super::checkpoint_times::Relation::Records.def()
    }
}

impl Related<Entity> for super::event_edition_records::Entity {
    fn to() -> RelationDef {
        super::event_edition_records::Relation::Records.def()
    }
}

impl Related<super::event_edition_records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionRecords.def()
    }
}

impl Related<super::maps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Maps.def()
    }
}

impl Related<super::players::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Players.def()
    }
}

impl ActiveModelBehavior for ActiveModel {
    fn before_save<'a, 'async_trait, C>(
        self,
        _db: &'a C,
        _insert: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Self, DbErr>> + Send + 'async_trait>>
    where
        C: ConnectionTrait,
        C: 'async_trait,
        'a: 'async_trait,
        Self: Send + 'async_trait,
    {
        Box::pin(ready(Err(DbErr::RecordNotInserted)))
    }

    fn before_delete<'a, 'async_trait, C>(
        self,
        _db: &'a C,
    ) -> Pin<Box<dyn Future<Output = Result<Self, DbErr>> + Send + 'async_trait>>
    where
        C: ConnectionTrait,
        C: 'async_trait,
        'a: 'async_trait,
        Self: Send + 'async_trait,
    {
        Box::pin(ready(Err(DbErr::RecordNotUpdated)))
    }
}
