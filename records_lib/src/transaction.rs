// TODO: remove this after testing
#![allow(missing_docs)]

use std::future::Future;

use crate::context::{HasPersistentMode, TransactionMode, WithTransactionMode};

type PoolConn<'a> = &'a mut sqlx::pool::PoolConnection<sqlx::MySql>;

pub trait TxnGuardFunc<'a, Mode, Param, C>:
    FnOnce(PoolConn<'a>, WithTransactionMode<C, Mode>, Param) -> Self::OutputFuture
{
    type OutputFuture: Future<Output = <Self as TxnGuardFunc<'a, Mode, Param, C>>::Output>;
    type Output;
}

impl<'a, F, Fut, Mode, Param, C> TxnGuardFunc<'a, Mode, Param, C> for F
where
    F: FnOnce(PoolConn<'a>, WithTransactionMode<C, Mode>, Param) -> Fut + ?Sized,
    Fut: Future,
{
    type OutputFuture = Fut;
    type Output = Fut::Output;
}

pub async fn within_transaction<F, Mode, Param, T, E, C>(
    mysql_conn: PoolConn<'_>,
    ctx: C,
    mode: Mode,
    args: Param,
    f: F,
) -> Result<T, E>
where
    F: for<'a> TxnGuardFunc<'a, Mode, Param, C, Output = Result<T, E>>,
    E: From<sqlx::Error>,
    C: HasPersistentMode,
    Mode: TransactionMode,
{
    let mut query = sqlx::QueryBuilder::new("start transaction ");
    mode.sql_push_txn_mode(&mut query)
        .build()
        .execute(&mut **mysql_conn)
        .await?;
    match f(mysql_conn, ctx.with_transaction_mode(mode), args).await {
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
