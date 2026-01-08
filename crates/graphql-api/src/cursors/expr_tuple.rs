use sea_orm::sea_query::SimpleExpr;

#[derive(Debug, Clone)]
pub enum ExprTuple {
    One(SimpleExpr),
    Two(SimpleExpr, SimpleExpr),
    Three(SimpleExpr, SimpleExpr, SimpleExpr),
    Many(Vec<SimpleExpr>),
}

pub trait IntoExprTuple {
    fn into_expr_tuple(self) -> ExprTuple;
}

impl<A: Into<SimpleExpr>> IntoExprTuple for (A,) {
    fn into_expr_tuple(self) -> ExprTuple {
        let (a,) = self;
        ExprTuple::One(a.into())
    }
}

impl<A, B> IntoExprTuple for (A, B)
where
    A: Into<SimpleExpr>,
    B: Into<SimpleExpr>,
{
    fn into_expr_tuple(self) -> ExprTuple {
        let (a, b) = self;
        ExprTuple::Two(a.into(), b.into())
    }
}

impl<A, B, C> IntoExprTuple for (A, B, C)
where
    A: Into<SimpleExpr>,
    B: Into<SimpleExpr>,
    C: Into<SimpleExpr>,
{
    fn into_expr_tuple(self) -> ExprTuple {
        let (a, b, c) = self;
        ExprTuple::Three(a.into(), b.into(), c.into())
    }
}

macro_rules! impl_into_expr_tuple {
    ($($gen:ident),*) => {
        impl<$($gen),*> IntoExprTuple for ($($gen),*)
        where
        $(
            $gen: Into<SimpleExpr>
        ),*
        {
            fn into_expr_tuple(self) -> ExprTuple {
                #[allow(non_snake_case)]
                let ($($gen),*) = self;
                ExprTuple::Many(vec![$($gen.into()),*])
            }
        }

    };
}

impl_into_expr_tuple!(A, B, C, D);
impl_into_expr_tuple!(A, B, C, D, E);
impl_into_expr_tuple!(A, B, C, D, E, F);
impl_into_expr_tuple!(A, B, C, D, E, F, G);
impl_into_expr_tuple!(A, B, C, D, E, F, G, H);
impl_into_expr_tuple!(A, B, C, D, E, F, G, H, I);
impl_into_expr_tuple!(A, B, C, D, E, F, G, H, I, K);
