use sea_orm::{EntityTrait, IntoIdentity, QuerySelect, Select, SelectModel, prelude::SeaRc};

use crate::cursors::query_builder::CursorQueryBuilder;

pub trait CursorPaginable {
    type Selector;

    fn paginate_cursor_by<C: IntoIdentity>(
        self,
        order_columns: C,
    ) -> CursorQueryBuilder<Self::Selector>;
}

impl<E: EntityTrait> CursorPaginable for Select<E> {
    type Selector = SelectModel<<E as EntityTrait>::Model>;

    fn paginate_cursor_by<C: IntoIdentity>(
        mut self,
        order_columns: C,
    ) -> CursorQueryBuilder<Self::Selector> {
        CursorQueryBuilder::new(
            QuerySelect::query(&mut self).take(),
            SeaRc::new(E::default()),
            order_columns,
        )
    }
}
