use sea_orm::entity::prelude::*;

/// Represents the time on a checkpoint on a map associated to a record.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "checkpoint_times")]
pub struct Model {
    /// The checkpoint number. It starts at 0.
    #[sea_orm(primary_key, auto_increment = false)]
    pub cp_num: u32,
    /// The ID of the related map.
    #[sea_orm(primary_key, auto_increment = false)]
    pub map_id: u32,
    /// The ID of the related record.
    #[sea_orm(primary_key, auto_increment = false)]
    pub record_id: u32,
    /// The time in milliseconds on this checkpoint.
    pub time: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::records::Entity",
        from = "Column::RecordId",
        to = "super::records::Column::RecordId",
        on_update = "Restrict",
        on_delete = "Cascade"
    )]
    Records,
}

impl Related<super::records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Records.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
