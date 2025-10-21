use async_graphql::InputObject;
use chrono::NaiveDateTime;

/// Filter options for querying records
#[derive(InputObject, Clone, Default)]
pub struct RecordsFilter {
    /// Filter by player login
    pub player_login: Option<String>,

    /// Filter by player name
    pub player_name: Option<String>,

    /// Filter by map UID
    pub map_uid: Option<String>,

    /// Filter by map name
    pub map_name: Option<String>,

    /// Filter records made before this date (ISO 8601 format)
    pub before_date: Option<NaiveDateTime>,

    /// Filter records made after this date (ISO 8601 format)
    pub after_date: Option<NaiveDateTime>,

    /// Filter records with time greater than this value (in milliseconds)
    pub time_gt: Option<i32>,

    /// Filter records with time less than this value (in milliseconds)
    pub time_lt: Option<i32>,
}
