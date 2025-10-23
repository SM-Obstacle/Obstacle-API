use async_graphql::InputObject;

/// Filter options for querying players
#[derive(InputObject, Clone, Default)]
pub struct PlayersFilter {
    /// Filter by player login
    pub player_login: Option<String>,

    /// Filter by player name
    pub player_name: Option<String>,
}
