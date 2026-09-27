use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mappack_maps")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub mappack_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub map_id: u32,
    pub map_order: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::mappacks::Entity",
        from = "Column::MappackId",
        to = "super::mappacks::Column::Id",
        on_delete = "Cascade"
    )]
    Mappacks,
    #[sea_orm(
        belongs_to = "super::maps::Entity",
        from = "Column::MapId",
        to = "super::maps::Column::Id",
        on_delete = "Cascade"
    )]
    Maps,
}

impl Related<super::mappacks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Mappacks.def()
    }
}

impl Related<super::maps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Maps.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
