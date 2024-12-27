use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    Ctx, HasEdition, HasEditionId, HasEvent, HasEventHandle, HasEventId, HasMap, HasMapId,
    HasMapUid, HasMappackId, HasMySqlPool, HasPlayer, HasPlayerId, HasPlayerLogin, HasRedisPool,
};

/// Context trait, used as a marker for context types that are not in an SQL transaction mode.
///
/// This trait is used in function signatures to type-check if we're not already in an SQL
/// transaction mode. It is implemented by all the context types, except the [`WithTransactionMode`] type.
///
/// See the [module documentation](super) for more information.
pub trait HasPersistentMode: Ctx {
    /// Empty function to make the various macro rules work.
    #[doc(hidden)]
    #[cold]
    fn __do_nothing(&self);
}

impl<T: HasPersistentMode> HasPersistentMode for &T {
    #[inline(always)]
    #[cold]
    fn __do_nothing(&self) {
        <T as HasPersistentMode>::__do_nothing(self);
    }
}

impl<T: HasPersistentMode> HasPersistentMode for &mut T {
    #[inline(always)]
    #[cold]
    fn __do_nothing(&self) {
        <T as HasPersistentMode>::__do_nothing(self);
    }
}

/// Trait used to identify the various modes of SQL-transactions.
///
/// See the [`Transactional`] trait documentation for more information.
pub trait TransactionMode: std::fmt::Debug + Copy + Send + Sync {
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

/// Context trait used to indicate that the context is currently in a database transaction mode.
///
/// For now, it only contains the transaction mode (whether it is in read only or read write mode).
///
/// See the [module documentation](super) for more information.
pub trait Transactional: Ctx {
    /// The transaction mode of the context. For example, this can be [`ReadWrite`] or [`ReadOnly`].
    type Mode: TransactionMode;
}

impl<T: Transactional> Transactional for &T {
    type Mode = <T as Transactional>::Mode;
}

impl<T: Transactional> Transactional for &mut T {
    type Mode = <T as Transactional>::Mode;
}

/// Marker context type used to wrap a context type and mark it as transactional.
///
/// This type removes the implementation of the [`HasPersistentMode`] trait.
///
/// This type is used with the [`within`](crate::transaction::within) function.
pub struct WithTransactionMode<E, M> {
    _mode: M,
    extra: E,
}

impl<E, M> WithTransactionMode<E, M> {
    pub(crate) fn new(extra: E, mode: M) -> Self {
        Self { extra, _mode: mode }
    }
}

impl<E, M> Transactional for WithTransactionMode<E, M>
where
    E: Ctx,
    M: TransactionMode,
{
    type Mode = M;
}

impl<E: Ctx, M: TransactionMode> Ctx for WithTransactionMode<E, M> {
    #[inline(always)]
    fn get_opt_event(&self) -> Option<&models::Event> {
        <E as Ctx>::get_opt_event(&self.extra)
    }

    #[inline(always)]
    fn get_opt_edition(&self) -> Option<&models::EventEdition> {
        <E as Ctx>::get_opt_edition(&self.extra)
    }

    #[inline(always)]
    fn get_opt_event_id(&self) -> Option<u32> {
        <E as Ctx>::get_opt_event_id(&self.extra)
    }

    #[inline(always)]
    fn get_opt_edition_id(&self) -> Option<u32> {
        <E as Ctx>::get_opt_edition_id(&self.extra)
    }
}

impl<E: HasRedisPool, M: TransactionMode> HasRedisPool for WithTransactionMode<E, M> {
    #[inline]
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasMySqlPool, M: TransactionMode> HasMySqlPool for WithTransactionMode<E, M> {
    #[inline]
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasPlayer, M: TransactionMode> HasPlayer for WithTransactionMode<E, M> {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerLogin, M: TransactionMode> HasPlayerLogin for WithTransactionMode<E, M> {
    #[inline]
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

impl<E: HasPlayerId, M: TransactionMode> HasPlayerId for WithTransactionMode<E, M> {
    #[inline]
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap, M: TransactionMode> HasMap for WithTransactionMode<E, M> {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapUid, M: TransactionMode> HasMapUid for WithTransactionMode<E, M> {
    #[inline]
    fn get_map_uid(&self) -> &str {
        <E as HasMapUid>::get_map_uid(&self.extra)
    }
}

impl<E: HasMapId, M: TransactionMode> HasMapId for WithTransactionMode<E, M> {
    #[inline]
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasMappackId, M: TransactionMode> HasMappackId for WithTransactionMode<E, M> {
    #[inline]
    fn get_mappack_id(&self) -> AnyMappackId<'_> {
        <E as HasMappackId>::get_mappack_id(&self.extra)
    }
}

impl<E: HasEvent, M: TransactionMode> HasEvent for WithTransactionMode<E, M> {
    #[inline]
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventHandle, M: TransactionMode> HasEventHandle for WithTransactionMode<E, M> {
    #[inline]
    fn get_event_handle(&self) -> &str {
        <E as HasEventHandle>::get_event_handle(&self.extra)
    }
}

impl<E: HasEventId, M: TransactionMode> HasEventId for WithTransactionMode<E, M> {
    #[inline]
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition, M: TransactionMode> HasEdition for WithTransactionMode<E, M> {
    #[inline]
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId, M: TransactionMode> HasEditionId for WithTransactionMode<E, M> {
    #[inline]
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}
