use entity::types;

/// The various rating kinds available for a map.
#[derive(PartialEq, Eq, Clone, Copy, async_graphql::Enum)]
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

impl From<types::RatingKind> for RatingKind {
    fn from(value: types::RatingKind) -> Self {
        match value {
            types::RatingKind::Route => Self::Route,
            types::RatingKind::Deco => Self::Deco,
            types::RatingKind::Smoothness => Self::Smoothness,
            types::RatingKind::Difficulty => Self::Difficulty,
        }
    }
}
