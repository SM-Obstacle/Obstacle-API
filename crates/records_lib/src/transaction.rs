//! A tiny module to make SQL transactions and wrap them with type-check.

use crate::{
    MySqlConnection,
    context::{HasPersistentMode, TransactionMode, WithTransactionMode},
};

/// Wraps the call of the provided function with an SQL transaction with the provided mode.
///
/// ## Arguments
///
/// * `mysql_conn`: the connection to the database, which is forwarded to the provided function.
/// * `ctx`: the API context, which needs to implement [`HasPersistentMode`], meaning it's
///   not already in transaction mode.
/// * `mode`: the transaction mode, it can be [`ReadOnly`][1] or [`ReadWrite`][2] for example.
/// * `f`: the function itself.
///
/// If you get some weird errors when passing the context with a borrow, try using the
/// [`assert_future_send`](crate::assert_future_send) function.
///
/// [1]: crate::context::ReadOnly
/// [2]: crate::context::ReadWrite
pub async fn within<F, Mode, T, E, C>(
    mysql_conn: MySqlConnection<'_>,
    ctx: C,
    mode: Mode,
    f: F,
) -> Result<T, E>
where
    F: for<'a> AsyncFnOnce(MySqlConnection<'a>, WithTransactionMode<C, Mode>) -> Result<T, E>,
    E: From<sqlx::Error>,
    C: HasPersistentMode,
    Mode: TransactionMode,
{
    let mut query = sqlx::QueryBuilder::new("start transaction ");
    mode.sql_push_txn_mode(&mut query)
        .build()
        .execute(&mut **mysql_conn)
        .await?;
    match f(mysql_conn, WithTransactionMode::new(ctx, mode)).await {
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
