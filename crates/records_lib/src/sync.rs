//! A tiny module to make SQL transactions and wrap them with type-check.

use std::fmt;

use sea_orm::{ConnectionTrait, DatabaseTransaction, DbErr, TransactionTrait};

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
pub async fn transaction_within<F, C, T, E>(conn: &C, f: F) -> Result<T, E>
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

/// Locks the given table for reading, during the execution of the provided closure, then unlocks
/// them.
pub async fn lock_reading_within<C, F, T, E, const N: usize>(
    conn: &C,
    table_names: [&str; N],
    f: F,
) -> Result<T, E>
where
    C: ConnectionTrait,
    F: AsyncFnOnce() -> Result<T, E>,
    E: From<DbErr>,
{
    struct FmtTableRead<'a>(&'a str);
    impl fmt::Display for FmtTableRead<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "`{}` READ", self.0)
        }
    }

    struct FmtJoinTablesRead<'a, const N: usize>([&'a str; N]);
    impl<const N: usize> fmt::Display for FmtJoinTablesRead<'_, N> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mut iter = self.0.iter();
            if let Some(table_name) = iter.next() {
                fmt::Display::fmt(&FmtTableRead(table_name), f)?;
                for table_name in iter {
                    f.write_str(", ")?;
                    fmt::Display::fmt(&FmtTableRead(table_name), f)?;
                }
            }
            Ok(())
        }
    }

    const {
        assert!(N > 0, "the table names array can't be empty");
    }

    conn.execute_unprepared(format!("LOCK TABLES {}", FmtJoinTablesRead(table_names)).as_str())
        .await?;
    let ret = f().await;
    conn.execute_unprepared("UNLOCK TABLES").await?;
    ret
}
