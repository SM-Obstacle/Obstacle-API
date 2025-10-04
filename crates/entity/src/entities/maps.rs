use sea_orm::entity::prelude::*;

/// A ShootMania Obstacle map in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "maps")]
pub struct Model {
    /// The map ID.
    #[sea_orm(primary_key)]
    pub id: u32,
    /// The map game UID (corresponds to the `Nod#Id` in the in-game script).
    #[sea_orm(unique)]
    pub game_id: String,
    /// The ID of the author of the map.
    pub player_id: u32,
    /// The name of the map.
    pub name: String,
    /// The amount of checkpoints on the map.
    ///
    /// This is optional because old maps may not have saved this info yet. If missing, it is
    /// updated when playing the map.
    pub cps_number: Option<u32>,
    /// The optional ID of the linked map.
    ///
    /// This was created for future features.
    pub linked_map: Option<u32>,

    /// The bronze time of the map.
    pub bronze_time: Option<i32>,
    /// The silver time of the map.
    pub silver_time: Option<i32>,
    /// The gold time of the map.
    pub gold_time: Option<i32>,
    /// The author time of the map.
    pub author_time: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "Entity",
        from = "Column::LinkedMap",
        to = "Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    SelfRef,
    #[sea_orm(has_many = "super::player_rating::Entity")]
    PlayerRating,
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "Restrict"
    )]
    Players,
    #[sea_orm(has_many = "super::rating::Entity")]
    Rating,
    #[sea_orm(has_many = "super::records::Entity")]
    Records,
}

impl Related<super::player_rating::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PlayerRating.def()
    }
}

impl Related<super::rating::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Rating.def()
    }
}

impl Related<super::records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Records.def()
    }
}

impl Related<super::players::Entity> for Entity {
    fn to() -> RelationDef {
        super::rating::Relation::Players.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::rating::Relation::Maps.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
