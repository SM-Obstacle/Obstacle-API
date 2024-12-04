#![allow(missing_docs)]

use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, Database, RedisPool};

macro_rules! impl_ctx {
    () => {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_event_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_edition_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_edition(&self) -> Option<(&models::Event, &models::EventEdition)> {
            <E as Ctx>::get_opt_event_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_edition_ids(&self) -> Option<(u32, u32)> {
            <E as Ctx>::get_opt_event_edition_ids(&self.extra)
        }
    };

    ($($t:tt)*) => { $($t)* };
}

macro_rules! new_combinator {
    (
        'combinator: struct $Combinator:ident $(<$lt:lifetime>)? {$(
            $field:ident: $ty:ty
        ),* $(,)?}

        $( 'trait $(needs [$($subtrait:ident),* $(,)?])?: $($trait_lt:lifetime)? trait $AssociatedTrait:ident.$trait_fn:ident -> $trait_fn_ty:ty |$trait_self:ident| {
            $expr:expr
        } )?

        'delegates {$(
            $($delegate_trait_lt:lifetime)? $DelegateTrait:ident.$fn:ident -> $fn_ty:ty
        ),* $(,)?}

        $( 'ctx_impl {$($ctx_impl_tt:tt)*} )?

        $( 'addon_impls {$(
            $($addon_trait_lt:lifetime)? $AddonTrait:ident.$addon_fn:ident -> $addon_fn_ty:ty |$addon_self:ident| {
                $addon_expr:expr
            }
        ),* $(,)?} )?
    ) => {
        pub struct $Combinator <$($lt,)? E> {
            $( $field: $ty, )*
            extra: E,
        }

        impl <$($lt,)? E: Ctx> Ctx for $Combinator <$($lt,)? E> {
            impl_ctx!($($($ctx_impl_tt)*)?);
        }

        $(
            pub trait $AssociatedTrait: Ctx $( $(+ $subtrait)* )? {
                fn $trait_fn(&self) -> $trait_fn_ty;
            }

            impl<T: $AssociatedTrait> $AssociatedTrait for &T {
                #[inline]
                fn $trait_fn(&self) -> $trait_fn_ty {
                    <T as $AssociatedTrait>::$trait_fn(self)
                }
            }

            impl<T: $AssociatedTrait> $AssociatedTrait for &mut T {
                #[inline]
                fn $trait_fn(&self) -> $trait_fn_ty {
                    <T as $AssociatedTrait>::$trait_fn(self)
                }
            }

            impl <$($trait_lt,)? E: Ctx> $AssociatedTrait for $Combinator <$($trait_lt,)? E> {
                fn $trait_fn($trait_self: &Self) -> $trait_fn_ty {
                    $expr
                }
            }
        )?

        $( $(
            impl <$($addon_trait_lt,)? E: Ctx> $AddonTrait for $Combinator <$($addon_trait_lt,)? E> {
                fn $addon_fn($addon_self: &Self) -> $addon_fn_ty {
                    $addon_expr
                }
            }
        )* )?

        $(
            impl <$($delegate_trait_lt,)? E: $DelegateTrait> $DelegateTrait for $Combinator <$($delegate_trait_lt,)? E> {
                #[inline]
                fn $fn(&self) -> $fn_ty {
                    <E as $DelegateTrait>::$fn(&self.extra)
                }
            }
        )*
    };
}

pub trait Ctx {
    #[inline(always)]
    fn by_ref(&self) -> &Self {
        self
    }

    #[inline(always)]
    fn by_ref_mut(&mut self) -> &mut Self {
        self
    }

    #[inline]
    fn get_opt_event(&self) -> Option<&models::Event> {
        None
    }

    #[inline]
    fn get_opt_event_id(&self) -> Option<u32> {
        None
    }

    #[inline]
    fn get_opt_edition(&self) -> Option<&models::EventEdition> {
        None
    }

    #[inline]
    fn get_opt_edition_id(&self) -> Option<u32> {
        None
    }

    #[inline]
    fn get_opt_event_edition(&self) -> Option<(&models::Event, &models::EventEdition)> {
        self.get_opt_event().zip(self.get_opt_edition())
    }

    #[inline]
    fn get_opt_event_edition_ids(&self) -> Option<(u32, u32)> {
        self.get_opt_event_id().zip(self.get_opt_edition_id())
    }

    #[inline]
    fn sql_frag_builder(&self) -> SqlFragmentBuilder<'_, Self>
    where
        Self: Sized,
    {
        SqlFragmentBuilder { ctx: self }
    }

    fn with_no_event(self) -> WithNoEvent<Self>
    where
        Self: Sized,
    {
        WithNoEvent { extra: self }
    }

    fn with_mappack(self, mappack_id: AnyMappackId<'_>) -> WithMappackId<'_, Self>
    where
        Self: Sized,
    {
        WithMappackId {
            mappack_id,
            extra: self,
        }
    }

    fn with_event_handle_owned(self, handle: String) -> WithEventHandleOwned<Self>
    where
        Self: Sized,
    {
        WithEventHandleOwned {
            handle,
            extra: self,
        }
    }

    fn with_event_handle(self, handle: &str) -> WithEventHandle<'_, Self>
    where
        Self: Sized,
    {
        WithEventHandle {
            handle,
            extra: self,
        }
    }

    fn with_map_uid_owned(self, uid: String) -> WithMapUidOwned<Self>
    where
        Self: Sized,
    {
        WithMapUidOwned { uid, extra: self }
    }

    fn with_map_uid(self, uid: &str) -> WithMapUid<'_, Self>
    where
        Self: Sized,
    {
        WithMapUid { uid, extra: self }
    }

    fn with_player_login_owned(self, login: String) -> WithPlayerLoginOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerLoginOwned { login, extra: self }
    }

    fn with_player_login(self, login: &str) -> WithPlayerLogin<'_, Self>
    where
        Self: Sized,
    {
        WithPlayerLogin { login, extra: self }
    }

    fn with_player_owned(self, player: models::Player) -> WithPlayerOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerOwned {
            player,
            extra: self,
        }
    }

    fn with_player(self, player: &models::Player) -> WithPlayer<'_, Self>
    where
        Self: Sized,
    {
        WithPlayer {
            player,
            extra: self,
        }
    }

    fn with_player_id(self, player_id: u32) -> WithPlayerId<Self>
    where
        Self: Sized,
    {
        WithPlayerId {
            player_id,
            extra: self,
        }
    }

    fn with_map_owned(self, map: models::Map) -> WithMapOwned<Self>
    where
        Self: Sized,
    {
        WithMapOwned { map, extra: self }
    }

    fn with_map(self, map: &models::Map) -> WithMap<'_, Self>
    where
        Self: Sized,
    {
        WithMap { map, extra: self }
    }

    fn with_map_id(self, map_id: u32) -> WithMapId<Self>
    where
        Self: Sized,
    {
        WithMapId {
            map_id,
            extra: self,
        }
    }

    fn with_pool(self, pool: Database) -> WithRedisPool<WithMySqlPool<Self>>
    where
        Self: Sized,
    {
        WithRedisPool {
            pool: pool.redis_pool,
            extra: WithMySqlPool {
                pool: pool.mysql_pool,
                extra: self,
            },
        }
    }

    fn with_mysql_pool(self, pool: MySqlPool) -> WithMySqlPool<Self>
    where
        Self: Sized,
    {
        WithMySqlPool { pool, extra: self }
    }

    fn with_redis_pool(self, pool: RedisPool) -> WithRedisPool<Self>
    where
        Self: Sized,
    {
        WithRedisPool { pool, extra: self }
    }

    fn with_event_owned(self, event: models::Event) -> WithEventOwned<Self>
    where
        Self: Sized,
    {
        WithEventOwned { event, extra: self }
    }

    fn with_event(self, event: &models::Event) -> WithEvent<'_, Self>
    where
        Self: Sized,
    {
        WithEvent { event, extra: self }
    }

    fn with_event_id(self, event_id: u32) -> WithEventId<Self>
    where
        Self: Sized,
    {
        WithEventId {
            event_id,
            extra: self,
        }
    }

    fn with_edition_owned(self, edition: models::EventEdition) -> WithEditionOwned<Self>
    where
        Self: Sized,
    {
        WithEditionOwned {
            edition,
            extra: self,
        }
    }

    fn with_edition(self, edition: &models::EventEdition) -> WithEdition<'_, Self>
    where
        Self: Sized,
    {
        WithEdition {
            edition,
            extra: self,
        }
    }

    fn with_edition_id(self, edition_id: u32) -> WithEditionId<Self>
    where
        Self: Sized,
    {
        WithEditionId {
            edition_id,
            extra: self,
        }
    }

    fn with_event_edition_owned(
        self,
        event: models::Event,
        edition: models::EventEdition,
    ) -> WithEventOwned<WithEditionOwned<Self>>
    where
        Self: Sized,
    {
        WithEventOwned {
            event,
            extra: WithEditionOwned {
                edition,
                extra: self,
            },
        }
    }

    fn with_event_edition<'a>(
        self,
        event: &'a models::Event,
        edition: &'a models::EventEdition,
    ) -> WithEvent<'a, WithEdition<'a, Self>>
    where
        Self: Sized,
    {
        WithEvent {
            event,
            extra: WithEdition {
                edition,
                extra: self,
            },
        }
    }

    fn with_event_ids(self, event_id: u32, edition_id: u32) -> WithEventId<WithEditionId<Self>>
    where
        Self: Sized,
    {
        WithEventId {
            event_id,
            extra: WithEditionId {
                edition_id,
                extra: self,
            },
        }
    }
}

impl<T: Ctx> Ctx for &T {}

impl<T: Ctx> Ctx for &mut T {}

new_combinator! {
    'combinator: struct WithMappackId<'a> {
        mappack_id: AnyMappackId<'a>,
    }
    'trait: 'a trait HasMappackId.get_mappack_id -> AnyMappackId<'_> |self| {
        self.mappack_id
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

new_combinator! {
    'combinator: struct WithEventHandle<'a> {
        handle: &'a str,
    }
    'trait: 'a trait HasEventHandle.get_event_handle -> &str |self| {
        self.handle
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
    'combinator: struct WithEventHandleOwned {
        handle: String,
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
        HasEventHandle.get_event_handle -> &str |self| {
            &self.handle
        }
    }
}

new_combinator! {
    'combinator: struct WithMapUid<'a> {
        uid: &'a str,
    }
    'trait: 'a trait HasMapUid.get_map_uid -> &str |self| {
        self.uid
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
    'combinator: struct WithMapUidOwned {
        uid: String,
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
        HasMapUid.get_map_uid -> &str |self| {
            &self.uid
        }
    }
}

new_combinator! {
    'combinator: struct WithPlayerLogin<'a> {
        login: &'a str,
    }
    'trait: 'a trait HasPlayerLogin.get_player_login -> &str |self| {
        self.login
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
    'combinator: struct WithPlayerLoginOwned {
        login: String,
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
        HasPlayerLogin.get_player_login -> &str |self| {
            &self.login
        }
    }
}

new_combinator! {
    'combinator: struct WithRedisPool {
        pool: RedisPool
    }
    'trait: trait HasRedisPool.get_redis_pool -> RedisPool |self| {
        self.pool.clone()
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
    'combinator: struct WithMySqlPool {
        pool: MySqlPool,
    }
    'trait: trait HasMySqlPool.get_mysql_pool -> MySqlPool |self| {
        self.pool.clone()
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

new_combinator! {
    'combinator: struct WithPlayer<'a> {
        player: &'a models::Player,
    }
    'trait needs [HasPlayerId, HasPlayerLogin]: 'a trait HasPlayer.get_player -> &models::Player |self| {
        self.player
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
        'a HasPlayerLogin.get_player_login -> &str |self| {
            &self.player.login
        },
        'a HasPlayerId.get_player_id -> u32 |self| {
            self.player.id
        }
    }
}

new_combinator! {
    'combinator: struct WithPlayerOwned {
        player: models::Player,
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
        HasPlayer.get_player -> &models::Player |self| {
            &self.player
        },
        HasPlayerLogin.get_player_login -> &str |self| {
            &self.player.login
        },
        HasPlayerId.get_player_id -> u32 |self| {
            self.player.id
        }
    }
}

new_combinator! {
    'combinator: struct WithPlayerId {
        player_id: u32,
    }
    'trait: trait HasPlayerId.get_player_id -> u32 |self| {
        self.player_id
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

new_combinator! {
    'combinator: struct WithMap<'a> {
        map: &'a models::Map,
    }
    'trait needs [HasMapId, HasMapUid]: 'a trait HasMap.get_map -> &models::Map |self| {
        self.map
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
        'a HasMapId.get_map_id -> u32 |self| {
            self.map.id
        },
        'a HasMapUid.get_map_uid -> &str |self| {
            &self.map.game_id
        },
    }
}

new_combinator! {
    'combinator: struct WithMapOwned {
        map: models::Map,
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
        HasMap.get_map -> &models::Map |self| {
            &self.map
        },
        HasMapId.get_map_id -> u32 |self| {
            self.map.id
        },
        HasMapUid.get_map_uid -> &str |self| {
            &self.map.game_id
        },
    }
}

new_combinator! {
    'combinator: struct WithMapId {
        map_id: u32,
    }
    'trait: trait HasMapId.get_map_id -> u32 |self| {
        self.map_id
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
    'combinator: struct WithEventId {
        event_id: u32,
    }
    'trait: trait HasEventId.get_event_id -> u32 |self| {
        self.event_id
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
            <E as Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            Some(self.event_id)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_edition_id(&self.extra)
        }
    }
}

new_combinator! {
    'combinator: struct WithEditionId {
        edition_id: u32,
    }
    'trait: trait HasEditionId.get_edition_id -> u32 |self| {
        self.edition_id
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
            <E as Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_event_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            Some(self.edition_id)
        }
    }
}

new_combinator! {
    'combinator: struct WithEvent<'a> {
        event: &'a models::Event,
    }
    'trait needs [HasEventHandle, HasEventId]: 'a trait HasEvent.get_event -> &models::Event |self| {
        self.event
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
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_edition_id(&self.extra)
        }
    }
    'addon_impls {
        'a HasEventHandle.get_event_handle -> &str |self| {
            &self.event.handle
        },
        'a HasEventId.get_event_id -> u32 |self| {
            self.event.id
        },
    }
}

new_combinator! {
    'combinator: struct WithEventOwned {
        event: models::Event,
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
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_edition_id(&self.extra)
        }
    }
    'addon_impls {
        HasEvent.get_event -> &models::Event |self| {
            &self.event
        },
        HasEventHandle.get_event_handle -> &str |self| {
            &self.event.handle
        },
        HasEventId.get_event_id -> u32 |self| {
            self.event.id
        },
    }
}

new_combinator! {
    'combinator: struct WithEdition<'a> {
        edition: &'a models::EventEdition,
    }
    'trait needs [HasEventId, HasEditionId]: 'a trait HasEdition.get_edition -> &models::EventEdition |self| {
        self.edition
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
            <E as Ctx>::get_opt_event(&self.extra)
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
        'a HasEditionId.get_edition_id -> u32 |self| {
            self.edition.id
        },
        'a HasEventId.get_event_id -> u32 |self| {
            self.edition.event_id
        },
    }
}

new_combinator! {
    'combinator: struct WithEditionOwned {
        edition:  models::EventEdition,
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
            <E as Ctx>::get_opt_event(&self.extra)
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
        HasEdition.get_edition -> &models::EventEdition |self| {
            &self.edition
        },
        HasEditionId.get_edition_id -> u32 |self| {
            self.edition.id
        },
        HasEventId.get_event_id -> u32 |self| {
            self.edition.event_id
        },
    }
}

new_combinator! {
    'combinator: struct WithNoEvent {
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

#[derive(Default)]
#[non_exhaustive]
pub struct Context {
    _priv: (),
}

impl Ctx for Context {}

pub struct SqlFragmentBuilder<'ctx, C> {
    ctx: &'ctx C,
}

impl<'ctx, C: Ctx> SqlFragmentBuilder<'ctx, C> {
    pub fn push_event_view_name<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB> {
        match self.ctx.get_opt_event_edition_ids() {
            Some(_) => qb.push("global_event_records "),
            None => qb.push("global_records "),
        }
        .push(label)
    }

    #[inline(always)]
    pub fn push_event_join<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
        records_label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB> {
        match self.ctx.get_opt_event_edition_ids() {
            Some(_) => qb
                .push("inner join event_edition_records ")
                .push(label)
                .push(" on ")
                .push(label)
                .push(".record_id = ")
                .push(records_label)
                .push(".record_id"),
            None => qb,
        }
    }

    #[inline(always)]
    pub fn push_event_filter<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB>
    where
        u32: sqlx::Encode<'args, DB> + sqlx::Type<DB>,
    {
        match self.ctx.get_opt_event_edition_ids() {
            Some((ev, ed)) => qb
                .push("and (")
                .push(label)
                .push(".event_id = ")
                .push_bind(ev)
                .push(" and ")
                .push(label)
                .push(".edition_id = ")
                .push_bind(ed)
                .push(")"),
            None => qb,
        }
    }
}
