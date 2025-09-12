use sea_orm::entity::prelude::*;

/// The association between events and their authors in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "event_admins")]
pub struct Model {
    /// The ID of the event.
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_id: u32,
    /// The ID of the player.
    #[sea_orm(primary_key, auto_increment = false)]
    pub player_id: u32,
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
        belongs_to = "super::players::Entity",
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Players,
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::players::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Players.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
