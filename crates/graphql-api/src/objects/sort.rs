use async_graphql::InputObject;

use crate::objects::{
    sort_order::SortOrder,
    sortable_fields::{MapRecordSortableField, UnorderedRecordSortableField},
};

#[derive(Debug, InputObject, Clone, Copy)]
pub(crate) struct UnorderedRecordSort {
    pub field: UnorderedRecordSortableField,
    pub order: Option<SortOrder>,
}

#[derive(Debug, InputObject, Clone, Copy)]
pub(crate) struct MapRecordSort {
    pub field: MapRecordSortableField,
    pub order: Option<SortOrder>,
}
