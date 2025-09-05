//! A tiny module to make SQL transactions and wrap them with type-check.

use sea_orm::{DatabaseTransaction, DbErr, TransactionTrait};

/// Wraps the call of the provided function with an SQL transaction with the provided mode.
///
/// ## Arguments
///
/// * `conn`: the connection to the database, which is forwarded to the provided function
///   as a transaction.
/// * `f`: the function itself.
///
/// If you get some weird errors, try wrapping the call using the
/// [`assert_future_send`](crate::assert_future_send) function.
pub async fn within<F, C, T, E>(conn: &C, f: F) -> Result<T, E>
where
    F: for<'a> AsyncFnOnce(&'a DatabaseTransaction) -> Result<T, E>,
    E: From<DbErr>,
    C: TransactionTrait,
{
    let txn = conn.begin().await?;

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
