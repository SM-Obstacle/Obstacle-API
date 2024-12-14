use crate::{mappack::AnyMappackId, models, Database, MySqlPool, RedisPool};

use super::{
    macros::new_combinator, transaction::HasPersistentMode, HasEdition, HasEditionId, HasEvent,
    HasEventHandle, HasEventId, HasMap, HasMapId, HasMapUid, HasMappackId, HasPlayer, HasPlayerId,
    HasPlayerLogin, Transactional,
};

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current Redis pool.
        ///
        /// Returned by the [`Ctx::with_redis_pool`](super::Ctx::with_redis_pool) method.
        struct WithRedisPool {
            pool: RedisPool
        }
    }
    'trait {
        /// Context trait used to retrieve the current Redis pool.
        ///
        /// See the [module documentation](super) for more information.
        trait HasRedisPool.get_redis_pool(self) -> RedisPool {
            self.pool.clone()
        }
    }
    'delegates {
        HasPersistentMode.__do_nothing -> (),

        // HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapId.get_map_id -> u32,
        HasMapUid.get_map_uid -> &str,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
}

impl<E: Transactional> Transactional for WithRedisPool<E> {
    type Mode = <E as Transactional>::Mode;
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current MariaDB pool.
        ///
        /// Returned by the [`Ctx::with_mysql_pool`](super::Ctx::with_mysql_pool) method.
        struct WithMySqlPool {
            pool: MySqlPool,
        }
    }
    'trait {
        /// Context trait used to retrieve the current MariaDB pool.
        ///
        /// See the [module documentation](super) for more information.
        trait HasMySqlPool.get_mysql_pool(self) -> MySqlPool {
            self.pool.clone()
        }
    }
    'delegates {
        HasPersistentMode.__do_nothing -> (),

        HasRedisPool.get_redis_pool -> RedisPool,
        // HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
}

impl<E: Transactional> Transactional for WithMySqlPool<E> {
    type Mode = <E as Transactional>::Mode;
}

/// Context trait used to retrieve all the database pools.
///
/// See the [module documentation](super) for more information.
pub trait HasDbPool: HasRedisPool + HasMySqlPool {
    /// Returns all the database pools as an instance of the [`Database`] type.
    fn get_pool(&self) -> Database {
        Database {
            mysql_pool: <Self as HasMySqlPool>::get_mysql_pool(self),
            redis_pool: <Self as HasRedisPool>::get_redis_pool(self),
        }
    }
}

impl<T: HasRedisPool + HasMySqlPool> HasDbPool for T {}
