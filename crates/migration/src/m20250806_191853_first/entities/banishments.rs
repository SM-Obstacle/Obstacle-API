use sea_orm::entity::prelude::*;

/// A banishment in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "banishments")]
pub struct Model {
    /// The ID of the ban.
    #[sea_orm(primary_key)]
    pub id: u32,
    /// The UTC date of the ban.
    pub date_ban: DateTime,
    /// The duration, in seconds, of the ban.
    ///
    /// If it's null, the ban is permanent.
    pub duration: Option<i64>,
    /// Equals true if the player was already banned before.
    pub was_reprieved: i8,
    /// The reason of the ban.
    pub reason: String,
    /// The ID of the banned player.
    pub player_id: Option<u32>,
    /// The ID of the player who banned them.
    pub banished_by: Option<u32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::BanishedBy",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    BanAuthor,
    #[sea_orm(
        belongs_to = "super::players::Entity",
        from = "Column::PlayerId",
        to = "super::players::Column::Id",
        on_update = "Restrict",
        on_delete = "SetNull"
    )]
    Player,
}

impl ActiveModelBehavior for ActiveModel {}
