use sea_orm::entity::prelude::*;

/// An event category in an event.
#[derive(Clone, Debug, Default, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_category")]
pub struct Model {
    /// The ID of the category.
    #[sea_orm(primary_key)]
    pub id: u32,
    /// The handle of the category.
    pub handle: String,
    /// The name of the category.
    pub name: String,
    /// The optional URL to the banner image.
    ///
    /// This is currently used in the Obstacle Titlepack menu.
    pub banner_img_url: Option<String>,
    /// The hex color of the category.
    ///
    /// For example, for the White category, it is "fff".
    pub hex_color: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::event_categories::Entity")]
    EventCategories,
    #[sea_orm(has_many = "super::event_edition_categories::Entity")]
    EventEditionCategories,
}

impl Related<super::event_categories::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventCategories.def()
    }
}

impl Related<super::event_edition_categories::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionCategories.def()
    }
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        super::event_categories::Relation::Event.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::event_categories::Relation::EventCategory.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
