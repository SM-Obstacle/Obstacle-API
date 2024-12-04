use crate::{mappack::AnyMappackId, models, Database, MySqlPool, RedisPool};

use super::{
    macros::new_combinator, HasEdition, HasEditionId, HasEvent, HasEventHandle, HasEventId, HasMap,
    HasMapId, HasMapUid, HasMappackId, HasPlayer, HasPlayerId, HasPlayerLogin,
};

new_combinator! {
    'combinator {
        struct WithRedisPool {
            pool: RedisPool
        }
    }
    'trait {
        trait HasRedisPool.get_redis_pool(self) -> RedisPool {
            self.pool.clone()
        }
    }
    'delegates {
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

new_combinator! {
    'combinator {
        struct WithMySqlPool {
            pool: MySqlPool,
        }
    }
    'trait {
        trait HasMySqlPool.get_mysql_pool(self) -> MySqlPool {
            self.pool.clone()
        }
    }
    'delegates {
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

pub trait HasDbPool: HasRedisPool + HasMySqlPool {
    fn get_pool(&self) -> Database {
        Database {
            mysql_pool: <Self as HasMySqlPool>::get_mysql_pool(self),
            redis_pool: <Self as HasRedisPool>::get_redis_pool(self),
        }
    }
}

impl<T: HasRedisPool + HasMySqlPool> HasDbPool for T {}
