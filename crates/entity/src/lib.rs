mod entities;
pub use entities::*;

pub mod current_bans;
pub mod global_event_records;
pub mod global_records;

pub mod prelude {
    pub use super::entities::prelude::*;

    pub use super::current_bans::Entity as CurrentBans;
    pub use super::global_event_records::Entity as GlobalEventRecords;
    pub use super::global_records::Entity as GlobalRecords;
}
