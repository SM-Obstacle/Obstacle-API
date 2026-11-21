use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mappack_player_periodic_ranking")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub period_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub mappack_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub player_id: u32,
    pub rank: u32,
    pub rank_average: f64,
    pub maps_finished: u32,
    pub worst_rank: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_delete = "Cascade"
    )]
    Players,
    #[sea_orm(
        belongs_to = "super::mappack_periodic_ranking::Entity",
        from = "(Column::PeriodId, Column::MappackId)",
        to = "(super::mappack_periodic_ranking::Column::PeriodId, super::mappack_periodic_ranking::Column::MappackId)",
        on_delete = "Cascade"
    )]
    MappackPeriodicRanking,
}

impl Related<super::players::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Players.def()
    }
}

impl Related<super::mappack_periodic_ranking::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MappackPeriodicRanking.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
