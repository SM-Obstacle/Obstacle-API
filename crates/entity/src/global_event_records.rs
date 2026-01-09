use std::{future::ready, pin::Pin};

use sea_orm::entity::prelude::*;

use crate::records;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "global_event_records")]
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
    pub event_id: u32,
    pub edition_id: u32,
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
    #[sea_orm(
        belongs_to = "super::event_edition::Entity",
        from = "(Column::EventId, Column::EditionId)",
        to = "(super::event_edition::Column::EventId, super::event_edition::Column::Id)",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    EventEdition,
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

impl Related<super::event_edition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEdition.def()
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

impl From<Model> for records::Model {
    fn from(value: Model) -> Self {
        Self {
            record_id: value.record_id,
            record_player_id: value.record_player_id,
            map_id: value.map_id,
            time: value.time,
            respawn_count: value.respawn_count,
            record_date: value.record_date,
            flags: value.flags,
            try_count: value.try_count,
            event_record_id: value.event_record_id,
            // TODO(#116): add mode version in global_event_records view
            modeversion: None,
            is_hidden: false,
        }
    }
}
