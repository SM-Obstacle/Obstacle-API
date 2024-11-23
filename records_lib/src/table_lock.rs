// TODO: remove this after testing
#![allow(missing_docs)]

use std::future::Future;

type PoolConn<'a> = &'a mut sqlx::pool::PoolConnection<sqlx::MySql>;

pub trait TableLockGuardFunc<'a, Param>: FnOnce(PoolConn<'a>, Param) -> Self::OutputFuture {
    type OutputFuture: Future<Output = <Self as TableLockGuardFunc<'a, Param>>::Output>;
    type Output;
}

impl<'a, F, Fut, Param> TableLockGuardFunc<'a, Param> for F
where
    F: FnOnce(PoolConn<'a>, Param) -> Fut + ?Sized,
    Fut: Future,
{
    type OutputFuture = Fut;
    type Output = Fut::Output;
}

pub async fn locked_records<F, Param, T, E>(
    mysql_conn: PoolConn<'_>,
    args: Param,
    f: F,
) -> Result<T, E>
where
    F: for<'a> TableLockGuardFunc<'a, Param, Output = Result<T, E>>,
    E: From<sqlx::Error>,
{
    sqlx::query("lock tables records as r read")
        .execute(&mut **mysql_conn)
        .await?;
    let ret = f(mysql_conn, args).await;
    sqlx::query("unlock tables")
        .execute(&mut **mysql_conn)
        .await?;
    ret
}
