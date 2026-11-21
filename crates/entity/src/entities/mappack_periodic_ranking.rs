use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mappack_periodic_ranking")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub period_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub mappack_id: String,
    pub event_id: Option<u32>,
    pub edition_id: Option<u32>,
    pub maps_count: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::mappack_ranking_period::Entity",
        from = "Column::PeriodId",
        to = "super::mappack_ranking_period::Column::PeriodId",
        on_delete = "Cascade"
    )]
    MappackRankingPeriod,
    #[sea_orm(has_many = "super::mappack_player_periodic_ranking::Entity")]
    PlayerScores,
    #[sea_orm(has_many = "super::mappack_map_periodic_ranking::Entity")]
    MapScores,
}

impl Related<super::mappack_ranking_period::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MappackRankingPeriod.def()
    }
}

impl Related<super::mappack_player_periodic_ranking::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PlayerScores.def()
    }
}

impl Related<super::mappack_map_periodic_ranking::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MapScores.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
