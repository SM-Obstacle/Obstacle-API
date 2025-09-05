use sea_orm::entity::prelude::*;

/// The role of a player in the database.
///
/// Roles of a player can be a simple Player, a Mod, or an Admin.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "role")]
pub struct Model {
    /// The ID of the role.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: u8,
    /// The name of the role.
    pub role_name: String,
    /// The privileges of the role, coded on 8 bits.
    ///
    /// Each bit represents a privilege. The privileges are not yet well defined, but
    /// the players have it with all zeros and the admins have it with all ones.
    pub privileges: Option<u8>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::players::Entity")]
    Players,
}

impl Related<super::players::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Players.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
