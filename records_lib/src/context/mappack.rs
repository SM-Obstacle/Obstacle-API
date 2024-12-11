use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    macros::new_combinator, HasEdition, HasEditionId, HasEvent, HasEventHandle, HasEventId, HasMap,
    HasMapId, HasMapUid, HasMySqlPool, HasPlayer, HasPlayerId, HasPlayerLogin, HasRedisPool,
};

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current mappack ID.
        ///
        /// Returned by the [`Ctx::with_mappack`](super::Ctx::with_mappack) method.
        struct WithMappackId<'a> {
            mappack_id: AnyMappackId<'a>,
        }
    }
    'trait {
        /// Context trait used to retrieve the current mappack ID.
        ///
        /// See the [module documentation](super) for more information.
        'a trait HasMappackId.get_mappack_id(self) -> AnyMappackId<'_> {
            self.mappack_id
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapId.get_map_id -> u32,
        'a HasMapUid.get_map_uid -> &str,

        // HasMappackId.get_mappack_id -> AnyMappackId<'_>,
        'a HasEvent.get_event -> &models::Event,
        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEventHandle.get_event_handle -> &str,
        'a HasEventId.get_event_id -> u32,
        'a HasEditionId.get_edition_id -> u32,
    }
}
