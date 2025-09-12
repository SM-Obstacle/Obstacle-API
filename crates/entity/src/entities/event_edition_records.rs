use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_edition_records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub record_id: u32,
    pub event_id: u32,
    pub edition_id: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::event_edition::Entity",
        from = "(Column::EventId, Column::EditionId)",
        to = "(super::event_edition::Column::EventId, super::event_edition::Column::Id)",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    EventEdition,
    #[sea_orm(
        belongs_to = "super::records::Entity",
        from = "Column::RecordId",
        to = "super::records::Column::RecordId",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Records,
}

impl Related<super::event_edition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEdition.def()
    }
}

impl Related<super::records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Records.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
