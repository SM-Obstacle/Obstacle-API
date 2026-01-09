use async_graphql::Enum;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum SortOrder {
    Ascending,
    Descending,
}

impl From<SortOrder> for sea_orm::Order {
    fn from(value: SortOrder) -> Self {
        match value {
            SortOrder::Ascending => sea_orm::Order::Asc,
            SortOrder::Descending => sea_orm::Order::Desc,
        }
    }
}
