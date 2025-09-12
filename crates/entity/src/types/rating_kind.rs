use sea_orm::{DbErr, FromQueryResult, TryGetable};

use crate::rating_kind;

/// The various rating kinds available for a map.
#[derive(serde::Serialize, PartialEq, Eq, Clone, Copy, Debug, async_graphql::Enum)]
#[non_exhaustive]
pub enum RatingKind {
    /// The rating of the route.
    Route,
    /// The rating of the decoration.
    Deco,
    /// The rating of the smoothness.
    Smoothness,
    /// The rating of the difficulty.
    Difficulty,
}

impl FromQueryResult for RatingKind {
    fn from_query_result(res: &sea_orm::QueryResult, pre: &str) -> Result<Self, DbErr> {
        let rating_kind = <rating_kind::Model as FromQueryResult>::from_query_result(res, pre)?;
        match (rating_kind.id, rating_kind.kind.as_str()) {
            (0, "route") => Ok(RatingKind::Route),
            (1, "deco") => Ok(RatingKind::Deco),
            (2, "smoothness") => Ok(RatingKind::Smoothness),
            (3, "difficulty") => Ok(RatingKind::Difficulty),
            (id, kind) => Err(DbErr::Type(format!(
                "Unknown rating_kind : ({id}, `{kind}`"
            ))),
        }
    }
}

impl TryGetable for RatingKind {
    fn try_get_by<I: sea_orm::ColIdx>(
        res: &sea_orm::QueryResult,
        _: I,
    ) -> Result<Self, sea_orm::TryGetError> {
        <Self as FromQueryResult>::from_query_result(res, "").map_err(From::from)
    }
}
