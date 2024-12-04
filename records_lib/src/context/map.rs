use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    macros::new_combinator, HasEdition, HasEditionId, HasEvent, HasEventHandle, HasEventId,
    HasMappackId, HasMySqlPool, HasPlayer, HasPlayerId, HasPlayerLogin, HasRedisPool,
};

new_combinator! {
    'combinator {
        struct WithMapUid<'a> {
            uid: &'a str,
        }
    }
    'trait {
        'a trait HasMapUid.get_map_uid(self) -> &str {
            self.uid
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        // HasMapUid.get_map_uid -> &str,
        'a HasMapId.get_map_id -> u32,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        'a HasEvent.get_event -> &models::Event,
        'a HasEventHandle.get_event_handle -> &str,
        'a HasEventId.get_event_id -> u32,

        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEditionId.get_edition_id -> u32,
    }
}

new_combinator! {
    'combinator {
        struct WithMapUidOwned {
            uid: String,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        // HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
    'addon_impls {
        HasMapUid.get_map_uid(self) -> &str {
            &self.uid
        }
    }
}

new_combinator! {
    'combinator {
        struct WithMap<'a> {
            map: &'a models::Map,
        }
    }
    'trait needs [HasMapId, HasMapUid] {
        'a trait HasMap.get_map(self) -> &models::Map {
            self.map
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        // HasMap.get_map -> &models::Map,
        // HasMapUid.get_map_uid -> &str,
        // HasMapId.get_map_id -> u32,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        'a HasEvent.get_event -> &models::Event,
        'a HasEventHandle.get_event_handle -> &str,
        'a HasEventId.get_event_id -> u32,

        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEditionId.get_edition_id -> u32,
    }
    'addon_impls {
        'a HasMapId.get_map_id(self) -> u32 {
            self.map.id
        },
        'a HasMapUid.get_map_uid(self) -> &str {
            &self.map.game_id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithMapOwned {
            map: models::Map,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        // HasMap.get_map -> &models::Map,
        // HasMapUid.get_map_uid -> &str,
        // HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
    'addon_impls {
        HasMap.get_map(self) -> &models::Map {
            &self.map
        },
        HasMapId.get_map_id(self) -> u32 {
            self.map.id
        },
        HasMapUid.get_map_uid(self) -> &str {
            &self.map.game_id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithMapId {
            map_id: u32,
        }
    }
    'trait {
        trait HasMapId.get_map_id(self) -> u32 {
            self.map_id
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerId.get_player_id -> u32,
        HasPlayerLogin.get_player_login -> &str,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        // HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
}
