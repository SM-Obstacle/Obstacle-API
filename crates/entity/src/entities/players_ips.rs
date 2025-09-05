use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "players_ips")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub player_id: u32,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Binary(32)")]
    pub ip_hash: Vec<u8>,
    pub ip_date: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
