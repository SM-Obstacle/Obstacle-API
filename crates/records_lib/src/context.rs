//! The context module.
//!
//! This module contains the [`Ctx`] trait and many other sub-traits for data forwarding following
//! the decorator pattern.
//!
//! The [`Ctx`] is used to group common data that are used accross various items. The data are given
//! via the `with_*` methods, like [`with_player_id`][1], following
//! the decorator pattern.
//!
//! Methods like [`with_player_id`][1] return a type that implements a trait for which `Ctx` is
//! the super-trait, which is [`HasPlayerId`] in this case. This trait can be used to constraint
//! a generic type in a function, to access the provided data from it, by using the
//! [`HasPlayerId::get_player_id`] method.
//!
//! Sub-traits themselves can be super-trait of anothers, like [`HasPlayer`]`: `[`HasPlayerId`]. This
//! allows to forward the context generic type implementing `HasPlayer` if the target function requires
//! a generic type implementing `HasPlayerId`. In other terms, if the context has a player, it has
//! a player ID (which is the player ID of the same player).
//!
//! [1]: Ctx::with_player_id

use sqlx::MySqlPool;

use crate::ModeVersion;
use crate::{Database, RedisPool, mappack::AnyMappackId, models};

mod macros;

mod event;
mod map;
mod mappack;
mod player;
mod pool;
mod transaction;

pub use self::event::*;
pub use self::map::*;
pub use self::mappack::*;
pub use self::player::*;
pub use self::pool::*;
pub use self::transaction::*;

/// The context trait.
///
/// See [the module documentation](self) for more information.
pub trait Ctx: Send + Sync {
    /// Borrows the context type, rather than consuming it.
    ///
    /// This is useful to allow applying adapters to the context while still having the ownership
    /// on it.
    #[inline(always)]
    fn by_ref(&self) -> &Self {
        self
    }

    /// Borrows mutably the context type, rather than consuming it.
    ///
    /// It is equivalent to the [`by_ref`](Ctx::by_ref) method.
    #[inline(always)]
    fn by_ref_mut(&mut self) -> &mut Self {
        self
    }

    /// Returns the optional event the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements [`HasEvent`].
    #[inline]
    fn get_opt_event(&self) -> Option<&models::Event> {
        None
    }

    /// Returns the optional event ID the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements [`HasEventId`].
    #[inline]
    fn get_opt_event_id(&self) -> Option<u32> {
        None
    }

    /// Returns the optional edition the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements [`HasEdition`].
    #[inline]
    fn get_opt_edition(&self) -> Option<&models::EventEdition> {
        None
    }

    /// Returns the optional edition ID the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements [`HasEditionId`].
    #[inline]
    fn get_opt_edition_id(&self) -> Option<u32> {
        None
    }

    /// Returns the current mode version used by the client.
    fn get_mode_version(&self) -> Option<ModeVersion> {
        None // this isn't working and always returns None
    }

    /// Returns the optional event and its edition the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements both
    /// [`HasEvent`] and [`HasEdition`].
    #[inline]
    fn get_opt_event_edition(&self) -> Option<(&models::Event, &models::EventEdition)> {
        self.get_opt_event().zip(self.get_opt_edition())
    }

    /// Returns the optional event ID and the edition ID the context has.
    ///
    /// By default, this returns `None`. It returns `Some(_)` if the context implements both
    /// [`HasEventId`] and [`HasEditionId`].
    #[inline]
    fn get_opt_event_edition_ids(&self) -> Option<(u32, u32)> {
        self.get_opt_event_id().zip(self.get_opt_edition_id())
    }

    /// Returns the SQL fragment builder based on this context.
    ///
    /// See the [`SqlFragmentBuilder`] documentation for more information.
    #[inline]
    fn sql_frag_builder(&self) -> SqlFragmentBuilder<'_, Self>
    where
        Self: Sized,
    {
        SqlFragmentBuilder { ctx: self }
    }

    /// Wraps this context with a type that removes the implementation of the event traits.
    ///
    /// When using this method, the context is considered to not have any event.
    fn with_no_event(self) -> WithNoEvent<Self>
    where
        Self: Sized,
    {
        WithNoEvent::new(self)
    }

    /// Wraps this context with a type containing the provided mappack ID.
    ///
    /// This type implements the [`HasMappackId`] trait.
    fn with_mappack(self, mappack_id: AnyMappackId<'_>) -> WithMappackId<'_, Self>
    where
        Self: Sized,
    {
        WithMappackId::new(self, mappack_id)
    }

    /// Wraps this context with a type containing the provided event handle.
    ///
    /// This type implements the [`HasEventHandle`] trait.
    fn with_event_handle_owned(self, handle: String) -> WithEventHandleOwned<Self>
    where
        Self: Sized,
    {
        WithEventHandleOwned::new(self, handle)
    }

    /// Wraps this context with a type containing the provided event handle.
    ///
    /// This type implements the [`HasEventHandle`] trait.
    fn with_event_handle(self, handle: &str) -> WithEventHandle<'_, Self>
    where
        Self: Sized,
    {
        WithEventHandle::new(self, handle)
    }

    /// Wraps this context with a type containing the provided map UID.
    ///
    /// This type implements the [`HasMapUid`] trait.
    fn with_map_uid_owned(self, uid: String) -> WithMapUidOwned<Self>
    where
        Self: Sized,
    {
        WithMapUidOwned::new(self, uid)
    }

    /// Wraps this context with a type containing the provided map UID.
    ///
    /// This type implements the [`HasMapUid`] trait.
    fn with_map_uid(self, uid: &str) -> WithMapUid<'_, Self>
    where
        Self: Sized,
    {
        WithMapUid::new(self, uid)
    }

    /// Wraps this context with a type containing the provided player login.
    ///
    /// This type implements the [`HasPlayerLogin`] trait.
    fn with_player_login_owned(self, login: String) -> WithPlayerLoginOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerLoginOwned::new(self, login)
    }

    /// Wraps this context with a type containing the provided player login.
    ///
    /// This type implements the [`HasPlayerLogin`] trait.
    fn with_player_login(self, login: &str) -> WithPlayerLogin<'_, Self>
    where
        Self: Sized,
    {
        WithPlayerLogin::new(self, login)
    }

    /// Wraps this context with a type containing the provided player.
    ///
    /// This type implements the [`HasPlayer`], [`HasPlayerId`], and [`HasPlayerLogin`] traits.
    fn with_player_owned(self, player: models::Player) -> WithPlayerOwned<Self>
    where
        Self: Sized,
    {
        WithPlayerOwned::new(self, player)
    }

    /// Wraps this context with a type containing the provided player.
    ///
    /// This type implements the [`HasPlayer`], [`HasPlayerId`], and [`HasPlayerLogin`] traits.
    fn with_player(self, player: &models::Player) -> WithPlayer<'_, Self>
    where
        Self: Sized,
    {
        WithPlayer::new(self, player)
    }

    /// Wraps this context with a type containing the provided player ID.
    ///
    /// This type implements the [`HasPlayerId`] trait.
    fn with_player_id(self, player_id: u32) -> WithPlayerId<Self>
    where
        Self: Sized,
    {
        WithPlayerId::new(self, player_id)
    }

    /// Wraps this context with a type containing the provided map.
    ///
    /// This type implements the [`HasMap`], [`HasMapId`], and [`HasMapUid`] traits.
    fn with_map_owned(self, map: models::Map) -> WithMapOwned<Self>
    where
        Self: Sized,
    {
        WithMapOwned::new(self, map)
    }

    /// Wraps this context with a type containing the provided map.
    ///
    /// This type implements the [`HasMap`], [`HasMapId`], and [`HasMapUid`] traits.
    fn with_map(self, map: &models::Map) -> WithMap<'_, Self>
    where
        Self: Sized,
    {
        WithMap::new(self, map)
    }

    /// Wraps this context with a type containing the provided map ID.
    ///
    /// This type implements the [`HasMapId`] trait.
    fn with_map_id(self, map_id: u32) -> WithMapId<Self>
    where
        Self: Sized,
    {
        WithMapId::new(self, map_id)
    }

    /// Wraps this context with a type containing the provided MySQL and Redis pools.
    ///
    /// This type implements the [`HasDbPool`], [`HasRedisPool`], and [`HasMySqlPool`] traits.
    fn with_pool(self, pool: Database) -> WithRedisPool<WithMySqlPool<Self>>
    where
        Self: Sized,
    {
        WithRedisPool::new(WithMySqlPool::new(self, pool.mysql_pool), pool.redis_pool)
    }

    /// Wraps this context with a type containing the provided MySQL pool.
    ///
    /// This type implements the [`HasMySqlPool`] trait.
    fn with_mysql_pool(self, pool: MySqlPool) -> WithMySqlPool<Self>
    where
        Self: Sized,
    {
        WithMySqlPool::new(self, pool)
    }

    /// Wraps this context with a type containing the provided Redis pool.
    ///
    /// This type implements the [`HasRedisPool`] trait.
    fn with_redis_pool(self, pool: RedisPool) -> WithRedisPool<Self>
    where
        Self: Sized,
    {
        WithRedisPool::new(self, pool)
    }

    /// Wraps this context with a type containing the provided event.
    ///
    /// This type implements the [`HasEvent`], [`HasEventId`], and [`HasEventHandle`] traits.
    fn with_event_owned(self, event: models::Event) -> WithEventOwned<Self>
    where
        Self: Sized,
    {
        WithEventOwned::new(self, event)
    }

    /// Wraps this context with a type containing the provided event.
    ///
    /// This type implements the [`HasEvent`], [`HasEventId`], and [`HasEventHandle`] traits.
    fn with_event(self, event: &models::Event) -> WithEvent<'_, Self>
    where
        Self: Sized,
    {
        WithEvent::new(self, event)
    }

    /// Wraps this context with a type containing the provided event ID.
    ///
    /// This type implements the [`HasEventId`] trait.
    fn with_event_id(self, event_id: u32) -> WithEventId<Self>
    where
        Self: Sized,
    {
        WithEventId::new(self, event_id)
    }

    /// Wraps this context with a type containing the provided event edition.
    ///
    /// This type implements the [`HasEdition`], [`HasEditionId`], and [`HasEventId`] traits.
    fn with_edition_owned(self, edition: models::EventEdition) -> WithEditionOwned<Self>
    where
        Self: Sized,
    {
        WithEditionOwned::new(self, edition)
    }

    /// Wraps this context with a type containing the provided event edition.
    ///
    /// This type implements the [`HasEdition`], [`HasEditionId`], and [`HasEventId`] traits.
    fn with_edition(self, edition: &models::EventEdition) -> WithEdition<'_, Self>
    where
        Self: Sized,
    {
        WithEdition::new(self, edition)
    }

    /// Wraps this context with a type containing the provided event edition ID.
    ///
    /// This type implements the [`HasEditionId`] trait.
    fn with_edition_id(self, edition_id: u32) -> WithEditionId<Self>
    where
        Self: Sized,
    {
        WithEditionId::new(self, edition_id)
    }

    /// Wraps this context with a type containing the provided event and edition.
    ///
    /// This type implements the [`HasEvent`], [`HasEventHandle`], [`HasEventId`],
    /// [`HasEdition`], and [`HasEditionId`] traits.
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

    /// Wraps this context with a type containing the provided event and edition.
    ///
    /// This type implements the [`HasEvent`], [`HasEventHandle`], [`HasEventId`],
    /// [`HasEdition`], and [`HasEditionId`] traits.
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

    /// Wraps this context with a type containing the provided event and edition ID.
    ///
    /// This type implements the [`HasEventId`] and [`HasEditionId`] traits.
    fn with_event_ids(self, event_id: u32, edition_id: u32) -> WithEventId<WithEditionId<Self>>
    where
        Self: Sized,
    {
        WithEventId::new(WithEditionId::new(self, edition_id), event_id)
    }
}

impl<T: Ctx> Ctx for &T {
    #[inline(always)]
    fn get_opt_event(&self) -> Option<&models::Event> {
        <T as Ctx>::get_opt_event(self)
    }

    #[inline(always)]
    fn get_opt_event_id(&self) -> Option<u32> {
        <T as Ctx>::get_opt_event_id(self)
    }

    #[inline(always)]
    fn get_opt_edition(&self) -> Option<&models::EventEdition> {
        <T as Ctx>::get_opt_edition(self)
    }

    #[inline(always)]
    fn get_opt_edition_id(&self) -> Option<u32> {
        <T as Ctx>::get_opt_edition_id(self)
    }

    #[inline(always)]
    fn get_mode_version(&self) -> Option<ModeVersion> {
        <T as Ctx>::get_mode_version(self)
    }
}

impl<T: Ctx> Ctx for &mut T {
    #[inline(always)]
    fn get_opt_event(&self) -> Option<&models::Event> {
        <T as Ctx>::get_opt_event(self)
    }

    #[inline(always)]
    fn get_opt_event_id(&self) -> Option<u32> {
        <T as Ctx>::get_opt_event_id(self)
    }

    #[inline(always)]
    fn get_opt_edition(&self) -> Option<&models::EventEdition> {
        <T as Ctx>::get_opt_edition(self)
    }

    #[inline(always)]
    fn get_opt_edition_id(&self) -> Option<u32> {
        <T as Ctx>::get_opt_edition_id(self)
    }

    #[inline(always)]
    fn get_mode_version(&self) -> Option<ModeVersion> {
        <T as Ctx>::get_mode_version(self)
    }
}

/// The default context type.
///
/// This is the base type that implements the [`Ctx`] trait. It doesn't have any data by default.
#[derive(Default)]
pub struct Context {
    /// The mode version used by the client.
    ///
    /// This mode version will be forwarded to other context types when using the methods of the
    /// [`Ctx`] trait. This means that for any context type, the returned version of the
    /// [`Ctx::get_mode_version`] method is the one provided here, or `None` if not provided
    /// (by using [`Context::default()`] for example).
    pub mode_version: Option<ModeVersion>,
}

impl Ctx for Context {
    fn get_mode_version(&self) -> Option<ModeVersion> {
        self.mode_version
    }
}

impl HasPersistentMode for Context {
    #[cold]
    #[doc(hidden)]
    #[inline(always)]
    fn __do_nothing(&self) {}
}

/// The SQL fragment builder, based on the context.
///
/// This type is retrieved by the [`Ctx::sql_frag_builder`] method.
pub struct SqlFragmentBuilder<'ctx, C> {
    ctx: &'ctx C,
}

impl<C: Ctx> SqlFragmentBuilder<'_, C> {
    /// Pushes the SQL view name to the provided query builder, based on the optional
    /// event the context has.
    ///
    /// It bounds the provided `label` argument to the view name.
    ///
    /// This is used because records made in an event context are retrieved from a different view
    /// than normal records.
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

    /// Pushes the SQL join fragment to the provided query builder, based on the optional
    /// event the context has.
    ///
    /// If the context has an event, it adds a join clause to the query with the table
    /// containing records made in an event context. The `label` argument is the label bound to this
    /// table, and `records_label` is the label that was previously bound to the normal records table.
    ///
    /// Otherwise, it doesn't do anything.
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

    /// Pushes an SQL condition filtering the results based on the optional event the context has.
    ///
    /// If the context has an event, it adds an SQL condition, starting with `AND ...`, that checks
    /// if the event ID and edition ID corresponds to those in the context.
    ///
    /// Otherwise, it doesn't do anything.
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
