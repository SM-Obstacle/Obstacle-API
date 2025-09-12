use sea_orm::entity::prelude::*;

/// A single rating of a player on a map.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "player_rating")]
pub struct Model {
    /// The ID of the player who rates.
    #[sea_orm(primary_key, auto_increment = false)]
    pub player_id: u32,
    /// The ID of the map.
    #[sea_orm(primary_key, auto_increment = false)]
    pub map_id: u32,
    /// The ID of the rating kind.
    #[sea_orm(primary_key, auto_increment = false)]
    pub kind: u8,
    /// The value of the rating, between 0 and 1.
    #[sea_orm(column_type = "Float")]
    pub rating: f32,
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
    #[sea_orm(
        belongs_to = "super::rating_kind::Entity",
        from = "Column::Kind",
        to = "super::rating_kind::Column::Id",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    RatingKind,
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

impl Related<super::rating_kind::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RatingKind.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
