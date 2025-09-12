use sea_orm::entity::prelude::*;

/// The association between event editions and their categories.
///
/// This association doesn't necessarily requires to also have an association between the same
/// categories and the event.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_edition_categories")]
pub struct Model {
    /// The ID of the event.
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_id: u32,
    /// The ID of the event edition.
    #[sea_orm(primary_key, auto_increment = false)]
    pub edition_id: u32,
    /// The ID of the category.
    #[sea_orm(primary_key, auto_increment = false)]
    pub category_id: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::event_category::Entity",
        from = "Column::CategoryId",
        to = "super::event_category::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    EventCategory,
    #[sea_orm(
        belongs_to = "super::event_edition::Entity",
        from = "(Column::EventId, Column::EditionId)",
        to = "(super::event_edition::Column::EventId, super::event_edition::Column::Id)",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    EventEdition,
    #[sea_orm(has_many = "super::event_edition_maps::Entity")]
    EventEditionMaps,
}

impl Related<super::event_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventCategory.def()
    }
}

impl Related<super::event_edition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEdition.def()
    }
}

impl Related<super::event_edition_maps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionMaps.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
