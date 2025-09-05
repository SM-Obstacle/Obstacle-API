use sea_orm::entity::prelude::*;

/// The association between event editions and their maps.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_edition_maps")]
pub struct Model {
    /// The ID of the event.
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_id: u32,
    /// The ID of the event edition.
    #[sea_orm(primary_key, auto_increment = false)]
    pub edition_id: u32,
    /// The ID of the map.
    #[sea_orm(primary_key, auto_increment = false)]
    pub map_id: u32,
    /// The optional ID of the category the map belongs.
    pub category_id: Option<u32>,
    /// The MX ID of the map.
    pub mx_id: Option<i64>,
    /// The 0-based index of the map to play in order in its category.
    pub order: u32,
    /// The ID of the original map.
    ///
    /// For example for a Benchmark 2 map with UID `X_benchmark`, this will likely be the ID of the map
    /// with UID `X`.
    pub original_map_id: Option<u32>,
    /// The MX ID of the original map.
    pub original_mx_id: Option<i64>,
    /// Whether to save a record from the original map to the event map.
    pub transitive_save: Option<i8>,
    /// The time of the bronze medal.
    pub bronze_time: Option<i32>,
    /// The time of the silver medal.
    pub silver_time: Option<i32>,
    /// The time of the gold medal.
    pub gold_time: Option<i32>,
    /// The time of the author medal.
    pub author_time: Option<i32>,
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
        belongs_to = "super::event_edition_categories::Entity",
        from = "(Column::EventId, Column::EditionId, Column::CategoryId)",
        to = "(super::event_edition_categories::Column::EventId, super::event_edition_categories::Column::EditionId, super::event_edition_categories::Column::CategoryId)",
        on_update = "Restrict",
        on_delete = "Restrict"
    )]
    EventEditionCategories,
    #[sea_orm(
        belongs_to = "super::maps::Entity",
        from = "Column::MapId",
        to = "super::maps::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Maps,
    #[sea_orm(
        belongs_to = "super::maps::Entity",
        from = "Column::OriginalMapId",
        to = "super::maps::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    OriginalMaps,
}

impl Related<super::event_edition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEdition.def()
    }
}

impl Related<super::event_edition_categories::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionCategories.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
