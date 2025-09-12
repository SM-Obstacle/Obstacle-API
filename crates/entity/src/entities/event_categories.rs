use sea_orm::entity::prelude::*;

/// The association between events and their categories.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_categories")]
pub struct Model {
    /// The ID of the event.
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_id: u32,
    /// The ID of the category.
    #[sea_orm(primary_key, auto_increment = false)]
    pub category_id: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::event::Entity",
        from = "Column::EventId",
        to = "super::event::Column::Id",
        on_update = "Restrict",
        on_delete = "Restrict"
    )]
    Event,
    #[sea_orm(
        belongs_to = "super::event_category::Entity",
        from = "Column::CategoryId",
        to = "super::event_category::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    EventCategory,
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::event_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventCategory.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
