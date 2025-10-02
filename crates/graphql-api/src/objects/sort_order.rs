use async_graphql::Enum;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum SortOrder {
    Ascending,
    Descending,
}
