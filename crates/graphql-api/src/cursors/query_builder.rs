use std::marker::PhantomData;

use sea_orm::{
    Condition, ConnectionTrait, DbErr, DynIden, FromQueryResult, Identity, IntoIdentity, Order,
    PartialModelTrait, QuerySelect, SelectModel, SelectorTrait,
    prelude::{Expr, SeaRc},
    sea_query::{ExprTrait as _, SelectStatement, SimpleExpr},
};

use crate::cursors::expr_tuple::{ExprTuple, IntoExprTuple};

/// An enhanced version of [`Cursor`][sea_orm::Cursor].
///
/// It allows specifying expressions for pagination values, instead of plain values only.
#[derive(Debug, Clone)]
pub struct CursorQueryBuilder<S> {
    query: SelectStatement,
    table: DynIden,
    order_columns: Identity,
    first: Option<u64>,
    last: Option<u64>,
    before: Option<ExprTuple>,
    after: Option<ExprTuple>,
    sort_asc: bool,
    is_result_reversed: bool,
    phantom: PhantomData<S>,
}

impl<S> CursorQueryBuilder<S> {
    /// Create a new cursor
    pub fn new<C>(query: SelectStatement, table: DynIden, order_columns: C) -> Self
    where
        C: IntoIdentity,
    {
        Self {
            query,
            table,
            order_columns: order_columns.into_identity(),
            last: None,
            first: None,
            after: None,
            before: None,
            sort_asc: true,
            is_result_reversed: false,
            phantom: PhantomData,
        }
    }

    /// Filter paginated result with corresponding column less than the input value
    pub fn before<V>(&mut self, values: V) -> &mut Self
    where
        V: IntoExprTuple,
    {
        self.before = Some(values.into_expr_tuple());
        self
    }

    /// Filter paginated result with corresponding column greater than the input value
    pub fn after<V>(&mut self, values: V) -> &mut Self
    where
        V: IntoExprTuple,
    {
        self.after = Some(values.into_expr_tuple());
        self
    }

    fn apply_filters(&mut self) -> &mut Self {
        if let Some(values) = self.after.clone() {
            let condition =
                self.apply_filter(values, |c, v| if self.sort_asc { c.gt(v) } else { c.lt(v) });
            self.query.cond_where(condition);
        }

        if let Some(values) = self.before.clone() {
            let condition =
                self.apply_filter(values, |c, v| if self.sort_asc { c.lt(v) } else { c.gt(v) });
            self.query.cond_where(condition);
        }

        self
    }

    fn apply_filter<F>(&self, values: ExprTuple, f: F) -> Condition
    where
        F: Fn(SimpleExpr, SimpleExpr) -> SimpleExpr,
    {
        let columns = match &self.order_columns {
            Identity::Unary(c1) => vec![c1],
            Identity::Binary(c1, c2) => vec![c1, c2],
            Identity::Ternary(c1, c2, c3) => vec![c1, c2, c3],
            Identity::Many(col_vec) => col_vec.iter().collect(),
        };

        let values = match values {
            ExprTuple::One(v1) => vec![v1],
            ExprTuple::Two(v1, v2) => vec![v1, v2],
            ExprTuple::Three(v1, v2, v3) => vec![v1, v2, v3],
            ExprTuple::Many(val_vec) => val_vec,
        };

        if columns.len() != values.len() {
            panic!("column arity mismatch");
        }

        let col_expr =
            |c| SimpleExpr::from(Expr::col((SeaRc::clone(&self.table), SeaRc::clone(c))));

        // The keyset predicate is expanded into an `OR` chain of `AND` groups:
        //
        //        c1 > v1
        //     OR (c1 = v1 AND c2 > v2)
        //     OR (c1 = v1 AND c2 = v2 AND c3 > v3)
        //
        // The row-tuple form `(c1, c2, c3) > (v1, v2, v3)` says the same thing far more
        // concisely, but MariaDB never turns it into an index range: it scans the whole
        // ordering index and filters afterwards, so every page costs O(table). That is
        // invisible on the first forward page and brutal when paginating backward, where
        // the scan starts from the far end of the index. The expanded form is sargable,
        // so the leading column drives a range scan instead.
        (0..columns.len()).fold(Condition::any(), |any, n| {
            let group = columns[..=n]
                .iter()
                .copied()
                .zip(values[..=n].iter())
                .enumerate()
                .fold(Condition::all(), |all, (i, (col, val))| {
                    let col = col_expr(col);
                    let val = val.clone();
                    all.add(if i == n { f(col, val) } else { col.eq(val) })
                });
            any.add(group)
        })
    }

    /// Use ascending sort order
    pub fn asc(&mut self) -> &mut Self {
        self.sort_asc = true;
        self
    }

    /// Use descending sort order
    pub fn desc(&mut self) -> &mut Self {
        self.sort_asc = false;
        self
    }

    /// Limit result set to only first N rows in ascending order of the order by column
    pub fn first(&mut self, num_rows: u64) -> &mut Self {
        self.last = None;
        self.first = Some(num_rows);
        self
    }

    /// Limit result set to only last N rows in ascending order of the order by column
    pub fn last(&mut self, num_rows: u64) -> &mut Self {
        self.first = None;
        self.last = Some(num_rows);
        self
    }

    fn resolve_sort_order(&mut self) -> Order {
        let should_reverse_order = self.last.is_some();
        self.is_result_reversed = should_reverse_order;

        if (self.sort_asc && !should_reverse_order) || (!self.sort_asc && should_reverse_order) {
            Order::Asc
        } else {
            Order::Desc
        }
    }

    fn apply_limit(&mut self) -> &mut Self {
        if let Some(num_rows) = self.first {
            self.query.limit(num_rows);
        } else if let Some(num_rows) = self.last {
            self.query.limit(num_rows);
        }

        self
    }

    fn apply_order_by(&mut self) -> &mut Self {
        self.query.clear_order_by();
        let ord = self.resolve_sort_order();

        let query = &mut self.query;
        let order = |query: &mut SelectStatement, col| {
            query.order_by((SeaRc::clone(&self.table), SeaRc::clone(col)), ord.clone());
        };
        match &self.order_columns {
            Identity::Unary(c1) => {
                order(query, c1);
            }
            Identity::Binary(c1, c2) => {
                order(query, c1);
                order(query, c2);
            }
            Identity::Ternary(c1, c2, c3) => {
                order(query, c1);
                order(query, c2);
                order(query, c3);
            }
            Identity::Many(vec) => {
                for col in vec.iter() {
                    order(query, col);
                }
            }
        }

        self
    }

    /// Construct a [Cursor] that fetch any custom struct
    pub fn into_model<M>(self) -> CursorQueryBuilder<SelectModel<M>>
    where
        M: FromQueryResult,
    {
        CursorQueryBuilder {
            query: self.query,
            table: self.table,
            order_columns: self.order_columns,
            last: self.last,
            first: self.first,
            after: self.after,
            before: self.before,
            sort_asc: self.sort_asc,
            is_result_reversed: self.is_result_reversed,
            phantom: PhantomData,
        }
    }

    /// Return a [Selector] from `Self` that wraps a [SelectModel] with a [PartialModel](PartialModelTrait)
    pub fn into_partial_model<M>(self) -> CursorQueryBuilder<SelectModel<M>>
    where
        M: PartialModelTrait,
    {
        M::select_cols(QuerySelect::select_only(self)).into_model::<M>()
    }
}

impl<S> CursorQueryBuilder<S>
where
    S: SelectorTrait,
{
    /// Fetch the paginated result
    pub async fn all<C>(&mut self, db: &C) -> Result<Vec<S::Item>, DbErr>
    where
        C: ConnectionTrait,
    {
        self.apply_limit();
        self.apply_order_by();
        self.apply_filters();

        let stmt = db.get_database_backend().build(&self.query);
        let rows = db.query_all(stmt).await?;
        let mut buffer = Vec::with_capacity(rows.len());
        for row in rows.into_iter() {
            buffer.push(S::from_raw_query_result(row)?);
        }
        if self.is_result_reversed {
            buffer.reverse()
        }
        Ok(buffer)
    }
}

impl<S> QuerySelect for CursorQueryBuilder<S> {
    type QueryStatement = SelectStatement;

    fn query(&mut self) -> &mut SelectStatement {
        &mut self.query
    }
}
