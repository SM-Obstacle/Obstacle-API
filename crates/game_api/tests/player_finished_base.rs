use entity::{global_event_records, global_records, records};

#[derive(Debug)]
pub struct Record {
    pub map_id: u32,
    pub player_id: u32,
    pub time: i32,
    pub respawn_count: i32,
    pub flags: u32,
}

macro_rules! impl_partial_eq {
    ($($entity:ident),*) => {
        $(
            impl PartialEq<Record> for $entity::Model {
                fn eq(&self, other: &Record) -> bool {
                    self.map_id == other.map_id
                        && self.record_player_id == other.player_id
                        && self.time == other.time
                        && self.respawn_count == other.respawn_count
                        && self.time == other.time
                        && self.flags == other.flags
                }
            }
        )*
    };
}

impl_partial_eq!(global_records, records, global_event_records);

#[derive(serde::Serialize)]
pub struct Request {
    pub map_uid: String,
    pub time: i32,
    pub respawn_count: i32,
    pub flags: Option<u32>,
    pub cps: Vec<i32>,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
pub struct Response {
    pub has_improved: bool,
    pub old: i32,
    pub new: i32,
    pub current_rank: i32,
    pub old_rank: i32,
}
