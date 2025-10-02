use async_graphql::Enum;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum UnorderedRecordSortableField {
    Date,
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum MapRecordSortableField {
    Date,
    Rank,
}
