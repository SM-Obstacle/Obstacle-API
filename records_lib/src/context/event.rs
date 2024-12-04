use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    macros::new_combinator, HasMap, HasMapId, HasMapUid, HasMappackId, HasMySqlPool, HasPlayer,
    HasPlayerId, HasPlayerLogin, HasRedisPool,
};

new_combinator! {
    'combinator {
        struct WithEventHandle<'a> {
            handle: &'a str,
        }
    }
    'trait {
        'a trait HasEventHandle.get_event_handle(self) -> &str {
            self.handle
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapUid.get_map_uid -> &str,
        'a HasMapId.get_map_id -> u32,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        'a HasEvent.get_event -> &models::Event,
        // HasEventHandle.get_event_handle -> &str,
        'a HasEventId.get_event_id -> u32,

        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEditionId.get_edition_id -> u32,
    }
}

new_combinator! {
    'combinator {
        struct WithEventHandleOwned {
            handle: String,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        // HasEventHandle.get_event_handle -> &str,
        HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
    'addon_impls {
        HasEventHandle.get_event_handle(self) -> &str {
            &self.handle
        }
    }
}

pub trait HasEventEdition: HasEvent + HasEdition {
    fn get_event(&self) -> (&models::Event, &models::EventEdition);
}

impl<T: HasEvent + HasEdition> HasEventEdition for T {
    fn get_event(&self) -> (&models::Event, &models::EventEdition) {
        (self.get_event(), self.get_edition())
    }
}

pub trait HasEventIds: HasEventId + HasEditionId {
    fn get_event_ids(&self) -> (u32, u32);
}

impl<T: HasEventId + HasEditionId> HasEventIds for T {
    fn get_event_ids(&self) -> (u32, u32) {
        (self.get_event_id(), self.get_edition_id())
    }
}

new_combinator! {
    'combinator {
        struct WithEventId {
            event_id: u32,
        }
    }
    'trait {
        trait HasEventId.get_event_id(self) -> u32 {
            self.event_id
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as super::Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.event_id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as super::Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as super::Ctx>::get_opt_edition_id(&self.extra)
        }
    }
}

new_combinator! {
    'combinator {
        struct WithEditionId {
            edition_id: u32,
        }
    }
    'trait {
        trait HasEditionId.get_edition_id(self) -> u32 {
            self.edition_id
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

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
        // HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as super::Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            <E as super::Ctx>::get_opt_event_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as super::Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            Some(self.edition_id)
        }
    }
}

new_combinator! {
    'combinator {
        struct WithEvent<'a> {
            event: &'a models::Event,
        }
    }
    'trait needs [HasEventHandle, HasEventId] {
        'a trait HasEvent.get_event(self) -> &models::Event {
            self.event
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapUid.get_map_uid -> &str,
        'a HasMapId.get_map_id -> u32,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        // HasEvent.get_event -> &models::Event,
        // HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        'a HasEdition.get_edition -> &models::EventEdition,
        'a HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            Some(self.event)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.event.id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as super::Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as super::Ctx>::get_opt_edition_id(&self.extra)
        }
    }
    'addon_impls {
        'a HasEventHandle.get_event_handle(self) -> &str {
            &self.event.handle
        },
        'a HasEventId.get_event_id(self) -> u32 {
            self.event.id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithEventOwned {
            event: models::Event,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        // HasEvent.get_event -> &models::Event,
        // HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        HasEdition.get_edition -> &models::EventEdition,
        HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            Some(&self.event)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.event.id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as super::Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as super::Ctx>::get_opt_edition_id(&self.extra)
        }
    }
    'addon_impls {
        HasEvent.get_event(self) -> &models::Event {
            &self.event
        },
        HasEventHandle.get_event_handle(self) -> &str {
            &self.event.handle
        },
        HasEventId.get_event_id(self) -> u32 {
            self.event.id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithEdition<'a> {
            edition: &'a models::EventEdition,
        }
    }
    'trait needs [HasEventId, HasEditionId] {
        'a trait HasEdition.get_edition(self) -> &models::EventEdition {
            self.edition
        }
    }
    'delegates {
        'a HasRedisPool.get_redis_pool -> RedisPool,
        'a HasMySqlPool.get_mysql_pool -> MySqlPool,

        'a HasPlayer.get_player -> &models::Player,
        'a HasPlayerLogin.get_player_login -> &str,
        'a HasPlayerId.get_player_id -> u32,

        'a HasMap.get_map -> &models::Map,
        'a HasMapUid.get_map_uid -> &str,
        'a HasMapId.get_map_id -> u32,

        'a HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        'a HasEvent.get_event -> &models::Event,
        'a HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        // HasEdition.get_edition -> &models::EventEdition,
        // HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as super::Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.edition.event_id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            Some(self.edition)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            Some(self.edition.id)
        }
    }
    'addon_impls {
        'a HasEditionId.get_edition_id(self) -> u32 {
            self.edition.id
        },
        'a HasEventId.get_event_id(self) -> u32 {
            self.edition.event_id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithEditionOwned {
            edition:  models::EventEdition,
        }
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        HasEvent.get_event -> &models::Event,
        HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        // HasEdition.get_edition -> &models::EventEdition,
        // HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as super::Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.edition.event_id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            Some(&self.edition)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            Some(self.edition.id)
        }
    }
    'addon_impls {
        HasEdition.get_edition(self) -> &models::EventEdition {
            &self.edition
        },
        HasEditionId.get_edition_id(self) -> u32 {
            self.edition.id
        },
        HasEventId.get_event_id(self) -> u32 {
            self.edition.event_id
        },
    }
}

new_combinator! {
    'combinator {
        struct WithNoEvent {}
    }
    'delegates {
        HasRedisPool.get_redis_pool -> RedisPool,
        HasMySqlPool.get_mysql_pool -> MySqlPool,

        HasPlayer.get_player -> &models::Player,
        HasPlayerLogin.get_player_login -> &str,
        HasPlayerId.get_player_id -> u32,

        HasMap.get_map -> &models::Map,
        HasMapUid.get_map_uid -> &str,
        HasMapId.get_map_id -> u32,

        HasMappackId.get_mappack_id -> AnyMappackId<'_>,

        // HasEvent.get_event -> &models::Event,
        // HasEventHandle.get_event_handle -> &str,
        // HasEventId.get_event_id -> u32,

        // HasEdition.get_edition -> &models::EventEdition,
        // HasEditionId.get_edition_id -> u32,
    }
    'ctx_impl {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            None
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            None
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            None
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            None
        }
    }
}
