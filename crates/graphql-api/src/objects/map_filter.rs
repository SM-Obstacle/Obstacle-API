use async_graphql::InputObject;

use crate::objects::player_filter::PlayersFilter;

/// Filter options for querying maps
#[derive(InputObject, Clone, Default)]
pub struct MapsFilter {
    /// Filter by map UID
    pub map_uid: Option<String>,

    /// Filter by map name
    pub map_name: Option<String>,

    /// Filter on the map author
    pub author: Option<PlayersFilter>,
}
