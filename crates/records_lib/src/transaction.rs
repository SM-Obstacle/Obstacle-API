//! A tiny module to make SQL transactions and wrap them with type-check.

use std::{fmt, marker::PhantomData};

use crate::MySqlConnection;

/// Trait used to identify the various modes of SQL-transactions.
///
/// See the [`Transactional`] trait documentation for more information.
pub trait TransactionMode: fmt::Debug + Copy + Send + Sync {
    /// Pushes the SQL query fragment that depends on this transaction mode.
    ///
    /// For example, the [`ReadOnly`] mode would write "`read only`".
    fn sql_push_txn_mode<'a, DB: sqlx::Database>(
        self,
        _: &'a mut sqlx::QueryBuilder<'a, DB>,
    ) -> &'a mut sqlx::QueryBuilder<'a, DB>;
}

/// The read-only transaction mode.
///
/// See [`TransactionMode`] for more information.
#[derive(Debug, Clone, Copy)]
pub struct ReadOnly;

impl TransactionMode for ReadOnly {
    fn sql_push_txn_mode<'a, DB: sqlx::Database>(
        self,
        query: &'a mut sqlx::QueryBuilder<'a, DB>,
    ) -> &'a mut sqlx::QueryBuilder<'a, DB> {
        query.push("read only")
    }
}

/// The read-write transaction mode.
///
/// See [`TransactionMode`] for more information.
#[derive(Debug, Clone, Copy)]
pub struct ReadWrite;

impl TransactionMode for ReadWrite {
    fn sql_push_txn_mode<'a, DB: sqlx::Database>(
        self,
        query: &'a mut sqlx::QueryBuilder<'a, DB>,
    ) -> &'a mut sqlx::QueryBuilder<'a, DB> {
        query.push("read write")
    }
}

pub trait SubSet<T> {}

impl<T> SubSet<T> for T {}

impl SubSet<ReadWrite> for ReadOnly {}

// Lifetime is faked to prevent any copy to outlive the original instance.
#[derive(Clone, Copy)]
pub struct TxnGuard<'a, M>(PhantomData<(&'a (), M)>);

/// Context trait used to indicate that the context is currently in a database transaction mode.
///
/// For now, it only contains the transaction mode (whether it is in read only or read write mode).
///
/// See the [module documentation](super) for more information.
pub trait Transactional {
    type Mode: TransactionMode;
}

impl<T> Transactional for &T
where
    T: Transactional,
{
    type Mode = <T as Transactional>::Mode;
}

impl<M> Transactional for TxnGuard<'_, M>
where
    M: TransactionMode,
{
    type Mode = M;
}

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
pub async fn within<F, Mode, T, E>(
    mysql_conn: MySqlConnection<'_>,
    mode: Mode,
    f: F,
) -> Result<T, E>
where
    F: for<'a, 'b> AsyncFnOnce(MySqlConnection<'a>, TxnGuard<'b, Mode>) -> Result<T, E>,
    E: From<sqlx::Error>,
    Mode: TransactionMode,
{
    let mut query = sqlx::QueryBuilder::new("start transaction ");
    mode.sql_push_txn_mode(&mut query)
        .build()
        .execute(&mut **mysql_conn)
        .await?;

    let guard = TxnGuard(PhantomData);

    match f(mysql_conn, guard).await {
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
