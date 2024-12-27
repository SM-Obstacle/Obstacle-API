//! A tiny module to make SQL transactions and wrap them with type-check.

use std::future::Future;

use crate::context::{HasPersistentMode, TransactionMode, WithTransactionMode};

type PoolConn<'a> = &'a mut sqlx::pool::PoolConnection<sqlx::MySql>;

/// An [`FnOnce`] which takes 3 arguments, returning a [`Future`].
///
/// This trait is used as a bound in the [`within_transaction`] function.
pub trait AsyncFnOnce<Arg0, Arg1, Arg2>: FnOnce(Arg0, Arg1, Arg2) -> Self::OutputFuture {
    /// The type of the returned future.
    type OutputFuture: Future<Output = <Self as AsyncFnOnce<Arg0, Arg1, Arg2>>::Output> + Send;
    /// The type of the output of the returned future.
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

/// Wraps the call of the provided function with an SQL transaction with the provided mode.
///
/// ## Arguments
///
/// * `mysql_conn`: the connection to the database, which is forwarded to the provided function.
/// * `ctx`: the API context, which needs to implement [`HasPersistentMode`], meaning it's
///   not already in transaction mode.
/// * `mode`: the transaction mode, it can be [`ReadOnly`][1] or [`ReadWrite`][2] for example.
/// * `param`: the object passed to the provided function as a parameter.
/// * `f`: the function itself.
///
/// If you get some weird errors when passing the context with a borrow, try using the
/// [`records_lib::assert_future_send`](crate::assert_future_send) function.
///
/// [1]: crate::context::ReadOnly
/// [2]: crate::context::ReadWrite
pub async fn within_transaction<F, Mode, Param, T, E, C>(
    mysql_conn: PoolConn<'_>,
    ctx: C,
    mode: Mode,
    param: Param,
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
    match f(mysql_conn, WithTransactionMode::new(ctx, mode), param).await {
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
