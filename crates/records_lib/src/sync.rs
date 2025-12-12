//! A tiny module to make SQL transactions and wrap them with type-check.

use sea_orm::{AccessMode, DatabaseTransaction, DbErr, IsolationLevel, TransactionTrait};

/// Wraps the call of the provided function with an SQL transaction with the provided mode.
///
/// ## Arguments
///
/// * `conn`: the connection to the database, which is forwarded to the provided function
///   as a transaction.
/// * `isolation_level`: the SQL isolation level.
/// * `access_mode`: the SQL access mode.
/// * `f`: the function itself.
///
/// If you get some weird errors, try wrapping the call using the
/// [`assert_future_send`](crate::assert_future_send) function.
pub async fn transaction_with_config<F, C, T, E>(
    conn: &C,
    isolation_level: Option<IsolationLevel>,
    access_mode: Option<AccessMode>,
    f: F,
) -> Result<T, E>
where
    F: for<'a> AsyncFnOnce(&'a DatabaseTransaction) -> Result<T, E>,
    E: From<DbErr>,
    C: TransactionTrait,
{
    let txn = conn.begin_with_config(isolation_level, access_mode).await?;

    match f(&txn).await {
        Ok(ret) => {
            txn.commit().await?;
            Ok(ret)
        }
        Err(e) => {
            txn.rollback().await?;
            Err(e)
        }
    }
}

/// Wraps the call of the provided function with an SQL transaction.
///
/// ## Arguments
///
/// * `conn`: the connection to the database, which is forwarded to the provided function
///   as a transaction.
/// * `f`: the function itself.
///
/// If you get some weird errors, try wrapping the call using the
/// [`assert_future_send`](crate::assert_future_send) function.
pub async fn transaction<F, C, T, E>(conn: &C, f: F) -> Result<T, E>
where
    F: for<'a> AsyncFnOnce(&'a DatabaseTransaction) -> Result<T, E>,
    E: From<DbErr>,
    C: TransactionTrait,
{
    transaction_with_config(conn, None, None, f).await
}
