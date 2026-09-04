use async_graphql::InputObject;

use crate::objects::string_filter::StringFilter;

/// Filter options for querying players
#[derive(InputObject, Clone, Default)]
pub struct PlayersFilter {
    /// Filter by player login
    pub player_login: Option<StringFilter>,

    /// Filter by player name
    pub player_name: Option<StringFilter>,
}
