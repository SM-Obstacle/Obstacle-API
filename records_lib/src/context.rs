#![allow(missing_docs)]

use sqlx::MySqlPool;

use crate::{mappack::AnyMappackId, models, Database, RedisPool};

mod macros;

mod event;
mod map;
mod mappack;
mod player;
mod pool;

pub use self::event::*;
pub use self::map::*;
pub use self::mappack::*;
pub use self::player::*;
pub use self::pool::*;

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
        WithNoEvent::new(self)
    }

    fn with_mappack(self, mappack_id: AnyMappackId<'_>) -> WithMappackId<'_, Self>
    where
        Self: Sized,
    {
        WithMappackId::new(self, mappack_id)
    }

    fn with_event_handle_owned(self, handle: String) -> WithEventHandleOwned<Self>
    where
        Self: Sized,
    {
        WithEventHandleOwned::new(self, handle)
    }

    fn with_event_handle(self, handle: &str) -> WithEventHandle<'_, Self>
    where
        Self: Sized,
    {
        WithEventHandle::new(self, handle)
    }

    fn with_map_uid_owned(self, uid: String) -> WithMapUidOwned<Self>
    where
        Self: Sized,
    {
        WithMapUidOwned::new(self, uid)
    }

    fn with_map_uid(self, uid: &str) -> WithMapUid<'_, Self>
    where
        Self: Sized,
    {
        WithMapUid::new(self, uid)
    }

    fn with_player_login_owned(self, login: String) -> WithPlayerLoginOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerLoginOwned::new(self, login)
    }

    fn with_player_login(self, login: &str) -> WithPlayerLogin<'_, Self>
    where
        Self: Sized,
    {
        WithPlayerLogin::new(self, login)
    }

    fn with_player_owned(self, player: models::Player) -> WithPlayerOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerOwned::new(self, player)
    }

    fn with_player(self, player: &models::Player) -> WithPlayer<'_, Self>
    where
        Self: Sized,
    {
        WithPlayer::new(self, player)
    }

    fn with_player_id(self, player_id: u32) -> WithPlayerId<Self>
    where
        Self: Sized,
    {
        WithPlayerId::new(self, player_id)
    }

    fn with_map_owned(self, map: models::Map) -> WithMapOwned<Self>
    where
        Self: Sized,
    {
        WithMapOwned::new(self, map)
    }

    fn with_map(self, map: &models::Map) -> WithMap<'_, Self>
    where
        Self: Sized,
    {
        WithMap::new(self, map)
    }

    fn with_map_id(self, map_id: u32) -> WithMapId<Self>
    where
        Self: Sized,
    {
        WithMapId::new(self, map_id)
    }

    fn with_pool(self, pool: Database) -> WithRedisPool<WithMySqlPool<Self>>
    where
        Self: Sized,
    {
        WithRedisPool::new(WithMySqlPool::new(self, pool.mysql_pool), pool.redis_pool)
    }

    fn with_mysql_pool(self, pool: MySqlPool) -> WithMySqlPool<Self>
    where
        Self: Sized,
    {
        WithMySqlPool::new(self, pool)
    }

    fn with_redis_pool(self, pool: RedisPool) -> WithRedisPool<Self>
    where
        Self: Sized,
    {
        WithRedisPool::new(self, pool)
    }

    fn with_event_owned(self, event: models::Event) -> WithEventOwned<Self>
    where
        Self: Sized,
    {
        WithEventOwned::new(self, event)
    }

    fn with_event(self, event: &models::Event) -> WithEvent<'_, Self>
    where
        Self: Sized,
    {
        WithEvent::new(self, event)
    }

    fn with_event_id(self, event_id: u32) -> WithEventId<Self>
    where
        Self: Sized,
    {
        WithEventId::new(self, event_id)
    }

    fn with_edition_owned(self, edition: models::EventEdition) -> WithEditionOwned<Self>
    where
        Self: Sized,
    {
        WithEditionOwned::new(self, edition)
    }

    fn with_edition(self, edition: &models::EventEdition) -> WithEdition<'_, Self>
    where
        Self: Sized,
    {
        WithEdition::new(self, edition)
    }

    fn with_edition_id(self, edition_id: u32) -> WithEditionId<Self>
    where
        Self: Sized,
    {
        WithEditionId::new(self, edition_id)
    }

    fn with_event_edition_owned(
        self,
        event: models::Event,
        edition: models::EventEdition,
    ) -> WithEventOwned<WithEditionOwned<Self>>
    where
        Self: Sized,
    {
        WithEventOwned::new(WithEditionOwned::new(self, edition), event)
    }

    fn with_event_edition<'a>(
        self,
        event: &'a models::Event,
        edition: &'a models::EventEdition,
    ) -> WithEvent<'a, WithEdition<'a, Self>>
    where
        Self: Sized,
    {
        WithEvent::new(WithEdition::new(self, edition), event)
    }

    fn with_event_ids(self, event_id: u32, edition_id: u32) -> WithEventId<WithEditionId<Self>>
    where
        Self: Sized,
    {
        WithEventId::new(WithEditionId::new(self, edition_id), event_id)
    }
}

impl<T: Ctx> Ctx for &T {}

impl<T: Ctx> Ctx for &mut T {}

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
