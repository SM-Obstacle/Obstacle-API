//! Contains all the models registered in the MySQL/MariaDB database.
//!
//! Because there isn't any complete ORM in Rust as in other languages (let's not talk about
//! [diesel](https://docs.rs/diesel)),
//! the types correspond to the raw tables in the database. This means that relations between models
//! are only represented by a foreign key like an ID.

use std::str::FromStr;

use async_graphql::{Enum, SimpleObject};
use serde::Serialize;
use sqlx::{FromRow, Row, mysql::MySqlRow};

/// A player in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Player {
    /// The player ID.
    pub id: u32,
    /// The player login.
    pub login: String,
    /// The player name.
    pub name: String,
    /// When the player played ShootMania Obstacle for the first time.
    pub join_date: Option<chrono::NaiveDateTime>,
    /// The player zone path.
    pub zone_path: Option<String>,
    /// An optional admins note.
    pub admins_note: Option<String>,
    /// The player role.
    ///
    /// See [`Role`] for the corresponding roles.
    pub role: u8,
}

/// A ShootMania Obstacle map in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Map {
    /// The map ID.
    pub id: u32,
    /// The map game UID (corresponds to the `Nod#Id` in the in-game script).
    pub game_id: String,
    /// The ID of the author of the map.
    pub player_id: u32,
    /// The name of the map.
    pub name: String,
    /// The amount of checkpoints on the map.
    ///
    /// This is optional because old maps may not have saved this info. If missing, it is
    /// updated when playing the map.
    pub cps_number: Option<u32>,
    /// The optional ID of the linked map.
    ///
    /// This was created for future features.
    pub linked_map: Option<u32>,
}

/// A record in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Record {
    /// The record ID.
    pub record_id: u32,
    /// The ID of the player who made the record.
    pub record_player_id: u32,
    /// The ID of the map.
    pub map_id: u32,
    /// The time in milliseconds of the run.
    pub time: i32,
    /// The amount of respawns.
    pub respawn_count: i32,
    /// The UTC date of the record.
    pub record_date: chrono::NaiveDateTime,
    /// The various flags of the run (Alt bug, fast respawn...)
    pub flags: u32,
    /// The amount of tries.
    ///
    /// This is optional as some old records don't have this info, and newest records neither, as it
    /// can be calculated since the Summer update.
    ///
    /// In the future, this may be set to the amount of full respawn of the player in the session.
    pub try_count: Option<u32>,
    /// Represents the ID of the record that this record was cloned from.
    ///
    /// When saving a record for an event, if the map has an original map, the record is cloned
    /// for this map, with this set to its ID. This helps to flag the cloned records
    /// in this context.
    pub event_record_id: Option<u32>,
}

/// A ranked record.
#[derive(Debug, Clone)]
pub struct RankedRecord {
    /// The rank of the record.
    pub rank: i32,
    /// The record itself.
    pub record: Record,
}

/// The role of a player in the database.
///
/// Roles of a player can be a simple Player, a Mod, or an Admin.
#[derive(Serialize, FromRow, Clone, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Role {
    /// The ID of the role.
    pub id: u8,
    /// The name of the role.
    pub role_name: String,
    /// The privileges of the role, coded on 8 bits.
    ///
    /// Each bit represents a privilege. The privileges are not yet well defined, but
    /// the players have it with all zeros and the admins have it with all ones.
    pub privileges: u8,
}

/// A banishment in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Banishment {
    /// The ID of the ban.
    pub id: u32,
    /// The UTC date of the ban.
    pub date_ban: chrono::NaiveDateTime,
    /// The duration, in seconds, of the ban.
    ///
    /// If it's null, the ban is permanent.
    pub duration: Option<i64>,
    /// The reason of the ban.
    pub reason: String,
    /// The ID of the banned player.
    pub player_id: Option<u32>,
    /// The ID of the player who banned them.
    pub banished_by: Option<u32>,
    /// Equals true if the player was already banned before.
    pub was_reprieved: bool,
}

/// Represents the time on a checkpoint on a map associated to a record.
#[derive(Serialize, FromRow, Clone, Debug, SimpleObject)]
pub struct CheckpointTime {
    /// The checkpoint number. It starts at 0.
    pub cp_num: u32,
    /// The ID of the related map.
    #[graphql(skip)]
    pub map_id: u32,
    /// The ID of the related record.
    #[graphql(skip)]
    pub record_id: u32,
    /// The time in milliseconds on this checkpoint.
    pub time: i32,
}

/// The rating kinds of a player on a map.
///
/// This isn't yet used in-game.
#[derive(Serialize, PartialEq, Eq, Clone, Copy, Debug, Enum)]
#[non_exhaustive]
pub enum RatingKind {
    /// The rating of the route.
    Route,
    /// The rating of the decoration.
    Deco,
    /// The rating of the smoothness.
    Smoothness,
    /// The rating of the difficulty.
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
                source: format!("unknown rating kind: `{id}`, `{kind}`").into(),
            }),
        }
    }
}

/// The "global" rating of a player on a map.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Rating {
    /// The ID of the player who rates.
    pub player_id: u32,
    /// The ID of the map.
    pub map_id: u32,
    /// The UTC date of the rating.
    pub rating_date: chrono::NaiveDateTime,
}

/// A single rating of a player on a map.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct PlayerRating {
    /// The ID of the player who rates.
    pub player_id: u32,
    /// The ID of the map.
    pub map_id: u32,
    /// The ID of the rating kind. See [`RatingKind`] for more information.
    pub kind: u8,
    /// The value of the rating, between 0 and 1.
    pub rating: f32,
}

/// An event in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Event {
    /// The ID of the event.
    pub id: u32,
    /// The event handle.
    pub handle: String,
    /// An optional cooldown for the event.
    ///
    /// This isn't used yet in the API.
    pub cooldown: Option<u8>,
}

/// The association between events and their authors in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct EventAdmins {
    /// The ID of the event.
    pub event_id: u32,
    /// The ID of the player.
    pub player_id: u32,
}

/// The association between events and their categories.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct EventCategories {
    /// The ID of the event.
    pub event_id: u32,
    /// The ID of the category. See [`EventCategory`] for more information.
    pub category_id: u32,
}

/// An event category in an event.
#[derive(Serialize, FromRow, Clone, Debug, SimpleObject, Default)]
pub struct EventCategory {
    /// The ID of the category.
    #[graphql(skip)]
    pub id: u32,
    /// The handle of the category.
    pub handle: String,
    /// The name of the category.
    pub name: String,
    /// The optional URL to the banner image.
    ///
    /// This is currently used in the Obstacle Titlepack menu.
    pub banner_img_url: Option<String>,
    /// The hex color of the category.
    ///
    /// For example, for the White category, it is "fff".
    pub hex_color: Option<String>,
}

/// The alignment of an item in the Titlepack menu.
#[derive(Serialize, Clone, Copy, Debug)]
#[repr(u8)]
#[serde(into = "char")]
pub enum InGameAlignment {
    /// The item is positioned on left.
    Left = b'L',
    /// The item is positioned on right.
    Right = b'R',
    /// The item is positioned on the center.
    Center = b'C',
}

impl From<InGameAlignment> for char {
    fn from(pos: InGameAlignment) -> Self {
        pos.to_char()
    }
}

impl InGameAlignment {
    fn try_from_char(c: char) -> Result<Self, sqlx::error::BoxDynError> {
        match c {
            'L' => Ok(Self::Left),
            'R' => Ok(Self::Right),
            'C' => Ok(Self::Center),
            c => Err(format!("invalid character: '{c}', expected 'L', 'R', or 'C'").into()),
        }
    }

    /// Converts an [`InGameAlignment`] into a character (either 'L' or 'R').
    pub fn to_char(self) -> char {
        self as u8 as char
    }
}

impl FromStr for InGameAlignment {
    type Err = sqlx::error::BoxDynError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.chars().next() {
            Some(c) if s.len() == 1 => Self::try_from_char(c),
            _ => Err("expected one character, either 'L', 'R', or 'C'"
                .to_owned()
                .into()),
        }
    }
}

impl<'a> sqlx::Decode<'a, sqlx::MySql> for InGameAlignment {
    fn decode(
        value: <sqlx::MySql as sqlx::Database>::ValueRef<'a>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <&'a str as sqlx::Decode<'a, sqlx::MySql>>::decode(value)?;
        Self::from_str(s)
    }
}

impl sqlx::Type<sqlx::MySql> for InGameAlignment {
    #[inline(always)]
    fn type_info() -> <sqlx::MySql as sqlx::Database>::TypeInfo {
        <str as sqlx::Type<sqlx::MySql>>::type_info()
    }

    #[inline(always)]
    fn compatible(ty: &<sqlx::MySql as sqlx::Database>::TypeInfo) -> bool {
        <str as sqlx::Type<sqlx::MySql>>::compatible(ty)
    }
}

/// Contains some additional parameters related to an [event edition](EventEdition).
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct InGameEventEditionParams {
    /// The ID of the additional parameters. This is just used as a foreign key by the event edition.
    pub id: i32,

    /// The boolean value of either to put the subtitle on a new line or not in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_subtitle_on_newline).
    pub put_subtitle_on_newline: Option<bool>,

    /// The alignment of the event edition titles.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_titles_align).
    pub titles_align: Option<InGameAlignment>,
    /// The alignment of the leaderboards link of the event edition.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_lb_link_align).
    pub lb_link_align: Option<InGameAlignment>,
    /// The alignment of the author list of the event edition.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_authors_align).
    pub authors_align: Option<InGameAlignment>,

    /// The X position of the event edition titles in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_titles_pos_x).
    pub titles_pos_x: Option<f64>,
    /// The Y position of the event edition titles in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_titles_pos_y).
    pub titles_pos_y: Option<f64>,
    /// The X position of the leaderboards link of the event edition in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_lb_link_pos_x).
    pub lb_link_pos_x: Option<f64>,
    /// The Y position of the leaderboards link of the event edition in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_lb_link_pos_y).
    pub lb_link_pos_y: Option<f64>,
    /// The X position of the author list of the event edition in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_authors_pos_x).
    pub authors_pos_x: Option<f64>,
    /// The Y position of the author list of the event edition in the Titlepack menu.
    ///
    /// The default value is defined [here](crate::LibEnv::ingame_default_authors_pos_y).
    pub authors_pos_y: Option<f64>,
}

/// An event edition in the database.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct EventEdition {
    /// The ID of the edition.
    ///
    /// This is a small number as it is always bound to the ID of the event.
    pub id: u32,
    /// The ID of the related event.
    pub event_id: u32,
    /// The name of the edition.
    pub name: String,
    /// The optional subtitle of the edition.
    ///
    /// For example, the 2nd campaign has the name "Winter" and the subtitle "2024".
    pub subtitle: Option<String>,
    /// The UTC start date of the edition.
    ///
    /// This date can be in the future.
    pub start_date: chrono::NaiveDateTime,
    /// The URL to the main banner image, used in the Titlepack menu, and on the website.
    pub banner_img_url: Option<String>,
    /// The URL to a small version to the previous banner image.
    ///
    /// This is shown in-game on the endscreen when finishing a map in the Campaign mode.
    pub banner2_img_url: Option<String>,
    /// The optional MX ID of the related mappack.
    pub mx_id: Option<i64>,
    /// The optional MX secret of the related mappack.
    pub mx_secret: Option<String>,
    /// The optional time-to-live of the event, in seconds.
    pub ttl: Option<u64>,
    /// Do we save records that weren't made in an event context for this event edition?
    pub save_non_event_record: bool,
    /// Don't we have any original map?
    ///
    /// This is used by the `global_records` view to retrieve the records of the events that don't
    /// have original maps.
    pub non_original_maps: bool,
    /// The foreign key to the in-game parameters of this event edition.
    ///
    /// The related model is [`InGameEventEditionParams`].
    pub ingame_params_id: Option<i32>,
    /// Whether the edition is transparent or not.
    ///
    /// A transparent event edition means that there is no records explicitly attached to this edition.
    /// Every record made on any map the edition contains is counted as being attached to this edition.
    pub is_transparent: bool,
}

/// The association between event editions and their categories.
///
/// This association doesn't necessarily requires to also have an association between the same
/// categories and the event.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct EventEditionCategories {
    /// The ID of the event.
    pub event_id: u32,
    /// The ID of the event edition.
    pub edition_id: u32,
    /// The ID of the category.
    pub category_id: u32,
}

/// The association between event editions and their maps.
#[derive(Serialize, FromRow, Clone, Debug)]
pub struct EventEditionMaps {
    /// The ID of the event.
    pub event_id: u32,
    /// The ID of the event edition.
    pub edition_id: u32,
    /// The ID of the map.
    pub map_id: u32,
    /// The optional ID of the category the map belongs.
    pub category_id: Option<u32>,
    /// The MX ID of the map.
    pub mx_id: Option<i64>,
    /// The ID of the original map.
    ///
    /// For example for a Benchmark map with UID `X_benchmark`, this will be the ID of the map
    /// with UID `X`.
    pub original_map_id: Option<u32>,
    /// The MX ID of the original map.
    pub original_mx_id: Option<i64>,
    /// Whether to save a record from the original map to the event map.
    pub transitive_save: Option<bool>,
    /// The time of the bronze medal.
    pub bronze_time: Option<i32>,
    /// The time of the silver medal.
    pub silver_time: Option<i32>,
    /// The time of the gold medal.
    pub gold_time: Option<i32>,
    /// The time of the author medal.
    pub author_time: Option<i32>,
}

/// The various status of the API.
#[derive(Serialize, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ApiStatusKind {
    /// The API is running normally.
    Normal,
    /// The API is currently in a maintenance mode.
    ///
    /// This happens when changing the structure of the database for example.
    Maintenance,
}

impl<'r> FromRow<'r, MySqlRow> for ApiStatusKind {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let id: u8 = row.try_get("status_id")?;
        let kind: String = row.try_get("status_name")?;

        match (id, &*kind) {
            (1, "normal") => Ok(Self::Normal),
            (2, "maintenance") => Ok(Self::Maintenance),
            (id, _) => Err(sqlx::Error::ColumnDecode {
                index: "status_id".to_owned(),
                source: format!("unknown status `{id}`, `{kind}`").into(),
            }),
        }
    }
}
