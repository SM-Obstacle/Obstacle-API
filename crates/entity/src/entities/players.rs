use sea_orm::entity::prelude::*;

/// A player in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "players")]
pub struct Model {
    /// The player ID.
    #[sea_orm(primary_key)]
    pub id: u32,
    /// The player login.
    #[sea_orm(unique)]
    pub login: String,
    /// The player name.
    pub name: String,
    /// When the player played ShootMania Obstacle for the first time.
    pub join_date: Option<DateTime>,
    /// The player zone path.
    pub zone_path: Option<String>,
    /// An optional note from an admin.
    pub admins_note: Option<String>,
    /// The player role.
    pub role: u8,
    /// The score of the player, calculated periodically.
    pub score: f64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::event_admins::Entity")]
    EventAdmins,
    #[sea_orm(has_many = "super::event_edition_admins::Entity")]
    EventEditionAdmins,
    #[sea_orm(has_many = "super::maps::Entity")]
    Maps,
    #[sea_orm(has_many = "super::player_rating::Entity")]
    PlayerRating,
    #[sea_orm(has_many = "super::rating::Entity")]
    Rating,
    #[sea_orm(has_many = "super::records::Entity")]
    Records,
    #[sea_orm(
        belongs_to = "super::role::Entity",
        from = "Column::Role",
        to = "super::role::Column::Id",
        on_update = "Restrict",
        on_delete = "Restrict"
    )]
    Role,
}

impl Related<super::event_admins::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventAdmins.def()
    }
}

impl Related<super::event_edition_admins::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventEditionAdmins.def()
    }
}

impl Related<super::player_rating::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PlayerRating.def()
    }
}

impl Related<super::rating::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Rating.def()
    }
}

impl Related<super::records::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Records.def()
    }
}

impl Related<super::role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Role.def()
    }
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        super::event_admins::Relation::Event.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::event_admins::Relation::Players.def().rev())
    }
}

impl Related<super::maps::Entity> for Entity {
    fn to() -> RelationDef {
        super::rating::Relation::Maps.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::rating::Relation::Players.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
