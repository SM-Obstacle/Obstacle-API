use sea_orm::entity::prelude::*;

use crate::types::InGameAlignment;

/// Contains some additional parameters related to an event edition.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "in_game_event_edition_params")]
pub struct Model {
    /// The ID of the additional parameters. This is just used as a foreign key by the event edition.
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The boolean value of either to put the subtitle on a new line or not in the Titlepack menu.
    pub put_subtitle_on_newline: Option<i8>,
    /// The alignment of the event edition titles.
    pub titles_align: Option<InGameAlignment>,
    /// The alignment of the leaderboards link of the event edition.
    pub lb_link_align: Option<InGameAlignment>,
    /// The alignment of the author list of the event edition.
    pub authors_align: Option<InGameAlignment>,
    /// The X position of the event edition titles in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub titles_pos_x: Option<f64>,
    /// The Y position of the event edition titles in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub titles_pos_y: Option<f64>,
    /// The X position of the leaderboards link of the event edition in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub lb_link_pos_x: Option<f64>,
    /// The Y position of the leaderboards link of the event edition in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub lb_link_pos_y: Option<f64>,
    /// The X position of the author list of the event edition in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub authors_pos_x: Option<f64>,
    /// The Y position of the author list of the event edition in the Titlepack menu.
    #[sea_orm(column_type = "Double", nullable)]
    pub authors_pos_y: Option<f64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::event_edition::Entity")]
    EventEdition,
}

impl Related<super::event_edition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEdition.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
