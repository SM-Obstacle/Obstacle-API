use async_graphql::Enum;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum UnorderedRecordSortableField {
    Date,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum MapRecordSortableField {
    Date,
    Rank,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum PlayerMapRankingSortableField {
    Name,
    Rank,
}
