use sea_orm::{
    IntoSimpleExpr,
    sea_query::{Func, SimpleExpr},
};

pub fn unstyled(v: impl IntoSimpleExpr) -> SimpleExpr {
    Func::cust("rm_mp_style").arg(v.into_simple_expr()).into()
}
