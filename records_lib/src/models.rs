use std::fmt;

use async_graphql::{Enum, SimpleObject};
use serde::Serialize;
use sqlx::{mysql::MySqlRow, FromRow, Row};

use crate::RecordsError;

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Player {
    pub id: u32,
    pub login: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub join_date: Option<chrono::NaiveDateTime>,
    pub country: String,
    pub admins_note: Option<String>,
    pub role: u8,
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct PlayerIps {
    pub player_id: u32,
    pub ip_hash: Vec<u8>,
    #[serde(skip_serializing)]
    pub ip_date: chrono::NaiveDateTime,
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Map {
    pub id: u32,
    pub game_id: String,
    pub player_id: u32,
    pub name: String,
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Record {
    pub id: u32,
    pub player_id: u32,
    pub map_id: u32,
    pub time: i32,
    pub respawn_count: i32,
    #[serde(skip_serializing)]
    pub record_date: chrono::NaiveDateTime,
    pub flags: u32,
    pub inputs_path: String,
    pub inputs_expiry: Option<u32>,
}

#[derive(Debug)]
pub struct RankedRecord {
    pub rank: i32,
    pub record: Record,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Enum)]
#[non_exhaustive]
pub enum Role {
    Player,
    Moderator,
    Admin,
}

impl<'r> FromRow<'r, MySqlRow> for Role {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let id: u8 = row.try_get("id")?;
        let role_name: String = row.try_get("role_name")?;

        match (id, &*role_name) {
            (0, "player") => Ok(Self::Player),
            (1, "mod") => Ok(Self::Moderator),
            (2, "admin") => Ok(Self::Admin),
            (id, _) => Err(sqlx::Error::ColumnDecode {
                index: "id".to_owned(),
                source: Box::new(RecordsError::UnknownRole(id, role_name)),
            }),
        }
    }
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct BanishmentInner {
    pub id: u32,
    #[serde(skip_serializing)]
    pub date_ban: chrono::NaiveDateTime,
    pub duration: Option<u32>,
    pub reason: Option<String>,
    pub player_id: u32,
    pub banished_by: u32,
}

#[derive(Serialize, Clone, Debug)]
pub struct Banishment {
    #[serde(flatten)]
    pub inner: BanishmentInner,
    pub was_reprieved: bool,
}

impl<'r> FromRow<'r, MySqlRow> for Banishment {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            inner: BanishmentInner::from_row(row)?,
            was_reprieved: row.try_get::<i8, _>("was_reprieved")? != 0,
        })
    }
}

impl fmt::Display for BanishmentInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "at: {:?}, duration: {}, reason: `{}`",
            self.date_ban,
            if let Some(d) = self.duration {
                format!("{d} seconds")
            } else {
                "forever".to_owned()
            },
            self.reason.as_deref().unwrap_or("none")
        )
    }
}

impl fmt::Display for Banishment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        <BanishmentInner as fmt::Display>::fmt(&self.inner, f)
    }
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Checkpoint {
    pub cp_num: u32,
    pub map_id: u32,
}

#[derive(Serialize, FromRow, Clone, Debug, SimpleObject)]
pub struct CheckpointTimes {
    pub cp_num: u32,
    #[graphql(skip)]
    pub map_id: u32,
    #[graphql(skip)]
    pub record_id: u32,
    pub time: i32,
}

#[derive(Serialize, PartialEq, Eq, Clone, Copy, Debug, Enum)]
pub enum Medal {
    Bronze,
    Silver,
    Gold,
    Champion,
}

impl<'r> FromRow<'r, MySqlRow> for Medal {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let id: u8 = row.try_get("id")?;
        let medal_name: String = row.try_get("medal_name")?;

        match (id, &*medal_name) {
            (0, "bronze") => Ok(Self::Bronze),
            (1, "silver") => Ok(Self::Silver),
            (2, "gold") => Ok(Self::Gold),
            (3, "champion") => Ok(Self::Champion),
            (id, _) => Err(sqlx::Error::ColumnDecode {
                index: "id".to_owned(),
                source: Box::new(RecordsError::UnknownMedal(id, medal_name)),
            }),
        }
    }
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct MedalPrice {
    pub id: u32,
    #[serde(skip_serializing)]
    pub price_date: chrono::NaiveDateTime,
    pub medal: u8,
    pub map_id: u32,
    pub player_id: u32,
}

#[derive(Serialize, PartialEq, Eq, Clone, Copy, Debug, Enum)]
pub enum RatingKind {
    Route,
    Deco,
    Smoothness,
    Difficulty,
}

impl<'r> FromRow<'r, MySqlRow> for RatingKind {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let id: u8 = row.try_get("id")?;
        let kind: String = row.try_get("kind")?;

        match (id, &*kind) {
            (0, "route") => Ok(Self::Route),
            (1, "deco") => Ok(Self::Deco),
            (2, "smoothness") => Ok(Self::Smoothness),
            (3, "difficulty") => Ok(Self::Difficulty),
            (id, _) => Err(sqlx::Error::ColumnDecode {
                index: "id".to_owned(),
                source: Box::new(RecordsError::UnknownRatingKind(id, kind)),
            }),
        }
    }
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Rating {
    pub player_id: u32,
    pub map_id: u32,
    #[serde(skip)]
    pub rating_date: chrono::NaiveDateTime,
    pub denominator: u8,
}

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct PlayerRating {
    pub player_id: u32,
    pub map_id: u32,
    pub kind: u8,
    pub rating: f32,
}
