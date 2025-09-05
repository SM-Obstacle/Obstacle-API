use sea_orm::{DbErr, FromQueryResult};

use crate::api_status;

/// The various status of the API.
#[derive(serde::Serialize, PartialEq, Eq, Clone, Copy, Debug, async_graphql::Enum)]
pub enum ApiStatusKind {
    /// The API is running normally.
    Normal,
    /// The API is currently in a maintenance mode.
    ///
    /// This happens when changing the structure of the database for example.
    Maintenance,
}

impl FromQueryResult for ApiStatusKind {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, sea_orm::DbErr> {
        let api_status = <api_status::Model as FromQueryResult>::from_query_result(res, pre)?;
        match (api_status.status_id, api_status.status_name.as_str()) {
            (1, "normal") => Ok(Self::Normal),
            (2, "maintenance") => Ok(Self::Maintenance),
            (id, kind) => Err(DbErr::Type(format!("Unknown api_status: ({id}, `{kind}`)"))),
        }
    }
}
