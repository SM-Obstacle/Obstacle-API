use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "mappack_ranking_period")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub period_id: i32,
    pub period_date: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::mappack_periodic_ranking::Entity")]
    MappackPeriodicRanking,
}

impl Related<super::mappack_periodic_ranking::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MappackPeriodicRanking.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
