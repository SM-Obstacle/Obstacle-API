use sea_orm::entity::prelude::*;

/// The "global" rating of a player on a map.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "rating")]
pub struct Model {
    /// The ID of the player who rates.
    #[sea_orm(primary_key, auto_increment = false)]
    pub player_id: u32,
    /// The ID of the map.
    #[sea_orm(primary_key, auto_increment = false)]
    pub map_id: u32,
    /// The UTC date of the rating.
    pub rating_date: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
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
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Players,
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

impl ActiveModelBehavior for ActiveModel {}
