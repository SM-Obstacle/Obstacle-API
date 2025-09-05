use sea_orm::entity::prelude::*;

/// An event edition in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_edition")]
pub struct Model {
    /// The ID of the edition.
    ///
    /// This is a small number as it is always bound to the ID of the event.
    #[sea_orm(primary_key)]
    pub id: u32,
    /// The ID of the related event.
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_id: u32,
    /// The name of the edition.
    pub name: String,
    /// The optional subtitle of the edition.
    ///
    /// For example, the 2nd campaign has the name "Winter" and the subtitle "2024".
    pub subtitle: Option<String>,
    /// The UTC start date of the edition.
    ///
    /// This date can be in the future.
    pub start_date: DateTime,
    /// The URL to the main banner image, used in the Titlepack menu, and on the website.
    pub banner_img_url: Option<String>,
    /// The URL to a small version of the previous banner image.
    ///
    /// This is shown in-game on the endscreen when finishing a map in the Campaign mode.
    pub banner2_img_url: Option<String>,
    /// The optional MX ID of the related mappack.
    pub mx_id: Option<i32>,
    /// The optional MX secret of the related mappack.
    pub mx_secret: Option<String>,
    /// The optional time-to-live of the event, in seconds.
    pub ttl: Option<u32>,
    /// Do we save records that weren't made in an event context for this event edition?
    pub save_non_event_record: i8,
    /// Flag to know if the edition doesn't have any original map.
    ///
    /// This is used by the `global_records` view to retrieve the records of the events that don't
    /// have original maps.
    pub non_original_maps: i8,
    /// The foreign key to the in-game parameters of this event edition.
    pub ingame_params_id: Option<i32>,
    /// Whether the edition is transparent or not.
    ///
    /// A transparent event edition means that there is no records explicitly attached to this edition.
    /// Every record made on any map the edition contains is counted as being attached to this edition.
    pub is_transparent: i8,
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
    #[sea_orm(has_many = "super::event_edition_admins::Entity")]
    EventEditionAdmins,
    #[sea_orm(has_many = "super::event_edition_categories::Entity")]
    EventEditionCategories,
    #[sea_orm(has_many = "super::event_edition_maps::Entity")]
    EventEditionMaps,
    #[sea_orm(has_many = "super::event_edition_records::Entity")]
    EventEditionRecords,
    #[sea_orm(
        belongs_to = "super::in_game_event_edition_params::Entity",
        from = "Column::IngameParamsId",
        to = "super::in_game_event_edition_params::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    InGameEventEditionParams,
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::event_edition_admins::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionAdmins.def()
    }
}

impl Related<super::event_edition_categories::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionCategories.def()
    }
}

impl Related<super::event_edition_maps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionMaps.def()
    }
}

impl Related<super::event_edition_records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionRecords.def()
    }
}

impl Related<super::in_game_event_edition_params::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InGameEventEditionParams.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
