mod entities;
pub use entities::*;

pub mod event_global_records;
pub mod global_records;

pub mod prelude {
    pub use super::entities::prelude::*;

    pub use super::event_admins::Entity as EventGlobalRecords;
    pub use super::global_records::Entity as GlobalRecords;
}
