// TODO: remove this after testing
#![allow(missing_docs)]

use std::future::Future;

use crate::context::{HasPersistentMode, TransactionMode, WithTransactionMode};

type PoolConn<'a> = &'a mut sqlx::pool::PoolConnection<sqlx::MySql>;

pub trait AsyncFnOnce<Arg0, Arg1, Arg2>: FnOnce(Arg0, Arg1, Arg2) -> Self::OutputFuture {
    type OutputFuture: Future<Output = <Self as AsyncFnOnce<Arg0, Arg1, Arg2>>::Output> + Send;
    type Output;
}

impl<F, Fut, Arg0, Arg1, Arg2> AsyncFnOnce<Arg0, Arg1, Arg2> for F
where
    F: FnOnce(Arg0, Arg1, Arg2) -> Fut + ?Sized,
    Fut: Future + Send,
{
    type OutputFuture = Fut;
    type Output = <Fut as Future>::Output;
}

pub async fn within_transaction<F, Mode, Param, T, E, C>(
    mysql_conn: PoolConn<'_>,
    ctx: C,
    mode: Mode,
    args: Param,
    f: F,
) -> Result<T, E>
where
    F: for<'a> AsyncFnOnce<
        PoolConn<'a>,
        WithTransactionMode<C, Mode>,
        Param,
        Output = Result<T, E>,
    >,
    E: From<sqlx::Error>,
    C: HasPersistentMode,
    Mode: TransactionMode,
{
    let mut query = sqlx::QueryBuilder::new("start transaction ");
    mode.sql_push_txn_mode(&mut query)
        .build()
        .execute(&mut **mysql_conn)
        .await?;
    match f(mysql_conn, WithTransactionMode::new(ctx, mode), args).await {
        Ok(ret) => {
            sqlx::query("commit").execute(&mut **mysql_conn).await?;
            Ok(ret)
        }
        Err(e) => {
            sqlx::query("rollback").execute(&mut **mysql_conn).await?;
            Err(e)
        }
    }
}
