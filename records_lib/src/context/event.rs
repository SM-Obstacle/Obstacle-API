use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, RedisPool};

use super::{
    macros::new_combinator, HasMap, HasMapId, HasMapUid, HasMappackId, HasMySqlPool, HasPlayer,
    HasPlayerId, HasPlayerLogin, HasRedisPool,
};

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current event handle.
        ///
        /// Returned by the [`Ctx::with_event_handle`](super::Ctx::with_event_handle) method.
        struct WithEventHandle<'a> {
            handle: &'a str,
        }
    }
    'trait {
        /// Context trait used to retrieve the current event handle.
        ///
        /// See the [module documentation](super) for more information.
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
        /// Adaptator context type used to contain the current event handle, with its ownership.
        ///
        /// Returned by the [`Ctx::with_event_handle_owned`](super::Ctx::with_event_handle_owned) method.
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

/// Context trait to get the current event and its edition.
///
/// See the [module documentation](super) for more information.
pub trait HasEventEdition: HasEvent + HasEdition {
    /// Returns a couple of references to the event and its edition.
    fn get_event(&self) -> (&models::Event, &models::EventEdition);
}

impl<T: HasEvent + HasEdition> HasEventEdition for T {
    fn get_event(&self) -> (&models::Event, &models::EventEdition) {
        (self.get_event(), self.get_edition())
    }
}

/// Context trait to get the current event ID and edition ID.
pub trait HasEventIds: HasEventId + HasEditionId {
    /// Returns a couple with the event ID and the edition ID.
    fn get_event_ids(&self) -> (u32, u32);
}

impl<T: HasEventId + HasEditionId> HasEventIds for T {
    fn get_event_ids(&self) -> (u32, u32) {
        (self.get_event_id(), self.get_edition_id())
    }
}

new_combinator! {
    'combinator {
        /// Adaptator context type used to contain the current event ID.
        ///
        /// Returned by the [`Ctx::with_event_id`](super::Ctx::with_event_id) method.
        struct WithEventId {
            event_id: u32,
        }
    }
    'trait {
        /// Context trait used to retrieve the current event ID.
        ///
        /// See the [module documentation](super) for more information.
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
        /// Adaptator context type used to contain the current edition ID.
        ///
        /// Returned by the [`Ctx::with_edition_id`](super::Ctx::with_edition_id) method.
        struct WithEditionId {
            edition_id: u32,
        }
    }
    'trait {
        /// Context trait used to retrieve the current edition ID.
        ///
        /// See the [module documentation](super) for more information.
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
        /// Adaptator context type used to contain a reference to the current event.
        ///
        /// Returned by the [`Ctx::with_event`](super::Ctx::with_event) method.
        struct WithEvent<'a> {
            event: &'a models::Event,
        }
    }
    'trait needs [HasEventHandle, HasEventId] {
        /// Context trait used to retrieve a reference to the current event.
        ///
        /// See the [module documentation](super) for more information.
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
        /// Adaptator context type used to contain the current event, with its ownership.
        ///
        /// Returned by the [`Ctx::with_event_owned`](super::Ctx::with_event_owned) method.
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
        /// Adaptator context type used to contain a reference to the current edition.
        ///
        /// Returned by the [`Ctx::with_edition`](super::Ctx::with_edition) method.
        struct WithEdition<'a> {
            edition: &'a models::EventEdition,
        }
    }
    'trait needs [HasEventId, HasEditionId] {
        /// Context trait used to retrieve a reference to the current edition.
        ///
        /// See the [module documentation](super) for more information.
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
        /// Adaptator context type used to contain the current edition, with its ownership.
        ///
        /// Returned by the [`Ctx::with_edition_owned`](super::Ctx::with_edition_owned) method.
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
        /// Adaptator context type used to remove any current event instance from the context.
        ///
        /// Returned by the [`Ctx::with_no_event`](super::Ctx::with_no_event) method.
        /// This type doesn't implement any trait related to events. It returns `None` for all the method
        /// implementations returning an optional event/edition.
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
