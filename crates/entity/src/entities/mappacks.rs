use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mappacks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub mx_author: Option<String>,
    pub mx_name: Option<String>,
    pub mx_created_at: Option<String>,
    pub last_updated_at: Option<DateTime>,
    pub expires_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::mappack_maps::Entity")]
    Maps,
}

impl Related<super::mappack_maps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Maps.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
