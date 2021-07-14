use serde::Serialize;
use sqlx::FromRow;

#[derive(Serialize, FromRow, Clone, Debug)]
pub struct Player {
    pub id: u32,
    pub login: String,
    pub name: String,
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
    pub try_count: i32,
    #[serde(skip_serializing)]
    pub created_at: chrono::NaiveDateTime,
    #[serde(skip_serializing)]
    pub updated_at: chrono::NaiveDateTime,
    pub flags: u32,
}

#[derive(Debug)]
pub struct RankedRecord {
    pub rank: i32,
    pub record: Record,
}
