use async_graphql::InputObject;
use chrono::NaiveDateTime;

use crate::objects::{map_filter::MapsFilter, player_filter::PlayersFilter};

/// Filter options for querying records
#[derive(InputObject, Clone, Default)]
pub struct RecordsFilter {
    /// Filter by player who made the record
    pub player: Option<PlayersFilter>,

    /// Filter by map on which the record is made
    pub map: Option<MapsFilter>,

    /// Filter records made before this date (ISO 8601 format)
    pub before_date: Option<NaiveDateTime>,

    /// Filter records made after this date (ISO 8601 format)
    pub after_date: Option<NaiveDateTime>,

    /// Filter records with time greater than this value (in milliseconds)
    pub time_gt: Option<i32>,

    /// Filter records with time less than this value (in milliseconds)
    pub time_lt: Option<i32>,
}
