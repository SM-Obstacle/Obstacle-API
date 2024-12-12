use crate::{mappack::AnyMappackId, models, MySqlPool, RedisPool};

use super::{
    macros::new_combinator, HasEdition, HasEditionId, HasEvent, HasEventHandle, HasEventId, HasMap,
    HasMapId, HasMapUid, HasMappackId, HasMySqlPool, HasRedisPool,
};

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current player login.
        ///
        /// Returned by the [`Ctx::with_player_login`](super::Ctx::with_player_login) method.
        struct WithPlayerLogin<'a> {
            login: &'a str,
        }
    }
    'trait {
        /// Context trait used to retrieve the current player login.
        ///
        /// See the [module documentation](super) for more information.
        'a trait HasPlayerLogin.get_player_login(self) -> &str {
            self.login
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        // HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapUid.get_map_uid -> &str,
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
        /// Adaptator context type used to contain the current player login, as an owned [`String`].
        ///
        /// Returned by the [`Ctx::with_player_login_owned`](super::Ctx::with_player_login_owned) method.
        struct WithPlayerLoginOwned {
            login: String,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        // HasPlayerLogin.get_player_login -> &str,
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
    'addon_impls {
        HasPlayerLogin.get_player_login(self) -> &str {
            &self.login
        }
    }
}

new_combinator! {
    'combinator {
        /// An adaptator context type used to contain a reference to the current player.
        ///
        /// Returned by the [`Ctx::with_player`](super::Ctx::with_player) method.
        struct WithPlayer<'a> {
            player: &'a models::Player,
        }
    }
    'trait needs [HasPlayerId, HasPlayerLogin] {
        /// Context trait used to retrieve a reference to the current player.
        ///
        /// See [the module documentation](super) for more information.
        'a trait HasPlayer.get_player(self) -> &models::Player {
            self.player
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        // HasPlayer.get_player -> &models::Player,
        // HasPlayerLogin.get_player_login -> &str,
        // HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapId.get_map_id -> u32,
        'a HasMapUid.get_map_uid -> &str,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        'a HasEvent.get_event -> &models::Event,
        'a HasEventHandle.get_event_handle -> &str,
        'a HasEventId.get_event_id -> u32,

        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEditionId.get_edition_id -> u32,
    }
    'addon_impls {
        'a HasPlayerLogin.get_player_login(self) -> &str {
            &self.player.login
        },
        'a HasPlayerId.get_player_id(self) -> u32 {
            self.player.id
        }
    }
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current player, with its ownership.
        ///
        /// Returned by the [`Ctx::with_player_owned`](super::Ctx::with_player_owned) method.
        struct WithPlayerOwned {
            player: models::Player,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        // HasPlayer.get_player -> &models::Player,
        // HasPlayerLogin.get_player_login -> &str,
        // HasPlayerId.get_player_id -> u32,

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
    'addon_impls {
        HasPlayer.get_player(self) -> &models::Player {
            &self.player
        },
        HasPlayerLogin.get_player_login(self) -> &str {
            &self.player.login
        },
        HasPlayerId.get_player_id(self) -> u32 {
            self.player.id
        }
    }
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current player ID.
        ///
        /// Returned by the [`Ctx::with_player_id`](super::Ctx::with_player_id) method.
        struct WithPlayerId {
            player_id: u32,
        }
    }
    'trait {
        /// Context trait used to retrieve the current player ID.
        ///
        /// See the [module documentation](super) for more information.
        trait HasPlayerId.get_player_id(self) -> u32 {
            self.player_id
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        // HasPlayerId.get_player_id -> u32,

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
