use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    macros::new_combinator, persistent::HasPersistentMode, HasEdition, HasEditionId, HasEvent,
    HasEventHandle, HasEventId, HasMappackId, HasMySqlPool, HasPlayer, HasPlayerId, HasPlayerLogin,
    HasRedisPool, Transactional,
};

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain a reference to the current map UID.
        ///
        /// Returned by the [`Ctx::with_map_uid`](super::Ctx::with_map_uid) method.
        struct WithMapUid<'a> {
            uid: &'a str,
        }
    }
    'trait {
        /// Context trait used to retrieve the current event handle.
        ///
        /// See the [module documentation](super) for more information.
        'a trait HasMapUid.get_map_uid(self) -> &str {
            self.uid
        }
    }
    'delegates {
        'a HasPersistentMode.__do_nothing -> (),

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

impl<E: Transactional> Transactional for WithMapUid<'_, E> {
    type Mode = <E as Transactional>::Mode;
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current map UID, as an owned [`String`].
        ///
        /// Returned by the [`Ctx::with_map_uid_owned`](super::Ctx::with_map_uid_owned) method.
        struct WithMapUidOwned {
            uid: String,
        }
    }
    'delegates {
        HasPersistentMode.__do_nothing -> (),

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

impl<E: Transactional> Transactional for WithMapUidOwned<E> {
    type Mode = <E as Transactional>::Mode;
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain a reference to the current map.
        ///
        /// Returned by the [`Ctx::with_map`](super::Ctx::with_map) method.
        struct WithMap<'a> {
            map: &'a models::Map,
        }
    }
    'trait needs [HasMapId, HasMapUid] {
        /// Context trait used to retrieve a reference to the current map.
        ///
        /// See the [module documentation](super) for more information.
        'a trait HasMap.get_map(self) -> &models::Map {
            self.map
        }
    }
    'delegates {
        'a HasPersistentMode.__do_nothing -> (),

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

impl<E: Transactional> Transactional for WithMap<'_, E> {
    type Mode = <E as Transactional>::Mode;
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current map, with its ownership.
        ///
        /// Returned by the [`Ctx::with_map_owned`](super::Ctx::with_map_owned) method.
        struct WithMapOwned {
            map: models::Map,
        }
    }
    'delegates {
        HasPersistentMode.__do_nothing -> (),

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

impl<E: Transactional> Transactional for WithMapOwned<E> {
    type Mode = <E as Transactional>::Mode;
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current map ID.
        struct WithMapId {
            map_id: u32,
        }
    }
    'trait {
        /// Context trait used to get the current map ID.
        ///
        /// See the [module documentation](super) for more information.
        trait HasMapId.get_map_id(self) -> u32 {
            self.map_id
        }
    }
    'delegates {
        HasPersistentMode.__do_nothing -> (),

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

impl<E: Transactional> Transactional for WithMapId<E> {
    type Mode = <E as Transactional>::Mode;
}
