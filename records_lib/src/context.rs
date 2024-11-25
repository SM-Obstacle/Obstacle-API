#![allow(missing_docs)]

use sqlx::MySqlPool;

use crate::{event::OptEvent, models, Database, DatabaseConnection, RedisConnection, RedisPool};

// considering N combination traits, for the N+1 trait, there are 2N new impls.
// (N delegation impls for the new trait and N new impls for each previous trait)
// so this is really big.
// TODO: maybe use a macro? :)

// TODO: add in the future: player login, map uid, mappack id, event handle (71 new impls)

pub trait Ctx {
    fn ctx(&self) -> &Context<'_>;

    #[inline(always)]
    fn by_ref(&self) -> &Self {
        self
    }

    #[inline(always)]
    fn by_ref_mut(&mut self) -> &mut Self {
        self
    }

    fn with_player_login(self, login: &str) -> WithPlayerLogin<'_, Self>
    where
        Self: Sized,
    {
        WithPlayerLogin { login, extra: self }
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

    fn with_db_conn<'a, 'b>(
        self,
        conn: &'a mut DatabaseConnection<'b>,
    ) -> WithDbConnection<'a, 'b, Self>
    where
        Self: Sized,
    {
        WithDbConnection { conn, extra: self }
    }

    fn with_mysql_conn(
        self,
        mysql_conn: &mut sqlx::MySqlConnection,
    ) -> WithMySqlConnection<'_, Self>
    where
        Self: Sized,
    {
        WithMySqlConnection {
            mysql_conn,
            extra: self,
        }
    }

    fn with_redis_conn(self, redis_conn: &mut RedisConnection) -> WithRedisConnection<'_, Self>
    where
        Self: Sized,
    {
        WithRedisConnection {
            redis_conn,
            extra: self,
        }
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

impl<T: Ctx> Ctx for &T {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <T as Ctx>::ctx(self)
    }
}

impl<T: Ctx> Ctx for &mut T {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <T as Ctx>::ctx(self)
    }
}

pub trait HasPlayerLogin: Ctx {
    fn get_player_login(&self) -> &str;
}

impl<T: HasPlayerLogin> HasPlayerLogin for &T {
    fn get_player_login(&self) -> &str {
        <T as HasPlayerLogin>::get_player_login(self)
    }
}

impl<T: HasPlayerLogin> HasPlayerLogin for &mut T {
    fn get_player_login(&self) -> &str {
        <T as HasPlayerLogin>::get_player_login(self)
    }
}

pub trait HasRedisPool: Ctx {
    fn get_redis_pool(&self) -> RedisPool;
}

impl<T: HasRedisPool> HasRedisPool for &T {
    fn get_redis_pool(&self) -> RedisPool {
        <T as HasRedisPool>::get_redis_pool(self)
    }
}

impl<T: HasRedisPool> HasRedisPool for &mut T {
    fn get_redis_pool(&self) -> RedisPool {
        <T as HasRedisPool>::get_redis_pool(self)
    }
}

pub trait HasMySqlPool: Ctx {
    fn get_mysql_pool(&self) -> MySqlPool;
}

impl<T: HasMySqlPool> HasMySqlPool for &T {
    fn get_mysql_pool(&self) -> MySqlPool {
        <T as HasMySqlPool>::get_mysql_pool(self)
    }
}

impl<T: HasMySqlPool> HasMySqlPool for &mut T {
    fn get_mysql_pool(&self) -> MySqlPool {
        <T as HasMySqlPool>::get_mysql_pool(self)
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

pub trait HasRedisConnection: Ctx {
    fn get_redis_conn(&mut self) -> &mut RedisConnection;
}

impl<T: HasRedisConnection> HasRedisConnection for &mut T {
    #[inline]
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <T as HasRedisConnection>::get_redis_conn(self)
    }
}

pub trait HasMySqlConnection: Ctx {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection;
}

impl<T: HasMySqlConnection> HasMySqlConnection for &mut T {
    #[inline]
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <T as HasMySqlConnection>::get_mysql_conn(self)
    }
}

pub trait HasDbConnection<'a>: HasMySqlConnection + HasRedisConnection {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a>;
}

impl<'a, T: HasDbConnection<'a>> HasDbConnection<'a> for &mut T {
    #[inline]
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <T as HasDbConnection<'a>>::get_db_conn(self)
    }
}

pub trait HasPlayer: Ctx {
    fn get_player(&self) -> &models::Player;
}

impl<T: HasPlayer> HasPlayer for &T {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <T as HasPlayer>::get_player(self)
    }
}

impl<T: HasPlayer> HasPlayer for &mut T {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <T as HasPlayer>::get_player(self)
    }
}

pub trait HasPlayerId: Ctx {
    fn get_player_id(&self) -> u32;
}

impl<T: HasPlayerId> HasPlayerId for &T {
    fn get_player_id(&self) -> u32 {
        <T as HasPlayerId>::get_player_id(self)
    }
}

impl<T: HasPlayerId> HasPlayerId for &mut T {
    fn get_player_id(&self) -> u32 {
        <T as HasPlayerId>::get_player_id(self)
    }
}

pub trait HasMap: Ctx {
    fn get_map(&self) -> &models::Map;
}

impl<T: HasMap> HasMap for &T {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <T as HasMap>::get_map(self)
    }
}

impl<T: HasMap> HasMap for &mut T {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <T as HasMap>::get_map(self)
    }
}

pub trait HasMapId: Ctx {
    fn get_map_id(&self) -> u32;
}

impl<T: HasMapId> HasMapId for &T {
    #[inline]
    fn get_map_id(&self) -> u32 {
        <T as HasMapId>::get_map_id(self)
    }
}

impl<T: HasMapId> HasMapId for &mut T {
    #[inline]
    fn get_map_id(&self) -> u32 {
        <T as HasMapId>::get_map_id(self)
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

pub trait HasEventId: Ctx {
    fn get_event_id(&self) -> u32;
}

impl<T: HasEventId> HasEventId for &T {
    fn get_event_id(&self) -> u32 {
        <T as HasEventId>::get_event_id(self)
    }
}

impl<T: HasEventId> HasEventId for &mut T {
    fn get_event_id(&self) -> u32 {
        <T as HasEventId>::get_event_id(self)
    }
}

pub trait HasEditionId: Ctx {
    fn get_edition_id(&self) -> u32;
}

impl<T: HasEditionId> HasEditionId for &T {
    fn get_edition_id(&self) -> u32 {
        <T as HasEditionId>::get_edition_id(self)
    }
}

impl<T: HasEditionId> HasEditionId for &mut T {
    fn get_edition_id(&self) -> u32 {
        <T as HasEditionId>::get_edition_id(self)
    }
}

pub trait HasEvent: Ctx {
    fn get_event(&self) -> &models::Event;
}

impl<T: HasEvent> HasEvent for &T {
    fn get_event(&self) -> &models::Event {
        <T as HasEvent>::get_event(self)
    }
}

impl<T: HasEvent> HasEvent for &mut T {
    fn get_event(&self) -> &models::Event {
        <T as HasEvent>::get_event(self)
    }
}

pub trait HasEdition: Ctx {
    fn get_edition(&self) -> &models::EventEdition;
}

impl<T: HasEdition> HasEdition for &T {
    fn get_edition(&self) -> &models::EventEdition {
        <T as HasEdition>::get_edition(self)
    }
}

impl<T: HasEdition> HasEdition for &mut T {
    fn get_edition(&self) -> &models::EventEdition {
        <T as HasEdition>::get_edition(self)
    }
}

#[derive(Default)]
#[non_exhaustive]
pub struct Context<'a> {
    pub event: OptEvent<'a, 'a>,
}

impl Ctx for Context<'_> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        self
    }
}

/**
 * In this module, impls are ordered in a way so that the first impls of each type
 * are the one made for the type, and the following impls are for delegation.
 *
 * This is just ordered like this so it's easier to refactor it by using a macro in the future.
 */

/**************************************
 * MAP IMPL
 **************************************/

pub struct WithMap<'a, E> {
    map: &'a models::Map,
    extra: E,
}

impl<E: Ctx> HasMap for WithMap<'_, E> {
    fn get_map(&self) -> &models::Map {
        self.map
    }
}

impl<E: Ctx> HasMapId for WithMap<'_, E> {
    fn get_map_id(&self) -> u32 {
        self.map.id
    }
}

impl<E: Ctx> Ctx for WithMap<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithMap<'_, E> {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithMap<'_, E> {
    #[inline]
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithMap<'_, E> {
    #[inline]
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithMap<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithMap<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithMap<'_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithMap<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithMap<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithMap<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithMap<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithMap<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithMap<'_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * MAP ID IMPL
 **************************************/

pub struct WithMapId<E> {
    map_id: u32,
    extra: E,
}

impl<E: Ctx> HasMapId for WithMapId<E> {
    fn get_map_id(&self) -> u32 {
        self.map_id
    }
}

impl<E: Ctx> Ctx for WithMapId<E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithMapId<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithMapId<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithMapId<E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithMapId<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithMapId<E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithMapId<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithMapId<E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithMapId<E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithMapId<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithMapId<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithMapId<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithMapId<E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithMapId<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * PLAYER IMPL
 **************************************/

pub struct WithPlayer<'a, E> {
    player: &'a models::Player,
    extra: E,
}

impl<E: Ctx> HasPlayer for WithPlayer<'_, E> {
    fn get_player(&self) -> &models::Player {
        self.player
    }
}

impl<E: Ctx> HasPlayerId for WithPlayer<'_, E> {
    fn get_player_id(&self) -> u32 {
        self.player.id
    }
}

impl<E: Ctx> HasPlayerLogin for WithPlayer<'_, E> {
    fn get_player_login(&self) -> &str {
        &self.player.login
    }
}

impl<E: Ctx> Ctx for WithPlayer<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithPlayer<'_, E> {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithPlayer<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithPlayer<'_, E> {
    #[inline]
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithPlayer<'_, E> {
    #[inline]
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithPlayer<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithPlayer<'_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithPlayer<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithPlayer<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithPlayer<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithPlayer<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithPlayer<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

/**************************************
 * PLAYER ID IMPL
 **************************************/

pub struct WithPlayerId<E> {
    player_id: u32,
    extra: E,
}

impl<E: Ctx> HasPlayerId for WithPlayerId<E> {
    fn get_player_id(&self) -> u32 {
        self.player_id
    }
}

impl<E: Ctx> Ctx for WithPlayerId<E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithPlayerId<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithPlayerId<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithPlayerId<E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithPlayerId<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithPlayerId<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithPlayerId<E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithPlayerId<E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithPlayerId<E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithPlayerId<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithPlayerId<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithPlayerId<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithPlayerId<E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithPlayerId<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * MYSQL CONNECTION IMPL
 **************************************/

pub struct WithMySqlConnection<'a, E> {
    mysql_conn: &'a mut sqlx::MySqlConnection,
    extra: E,
}

impl<E: Ctx> HasMySqlConnection for WithMySqlConnection<'_, E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        self.mysql_conn
    }
}

impl<E: Ctx> Ctx for WithMySqlConnection<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithMySqlConnection<'_, E> {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithMySqlConnection<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithMySqlConnection<'_, E> {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithMySqlConnection<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithMySqlConnection<'_, E> {
    #[inline]
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithMySqlConnection<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithMySqlConnection<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithMySqlConnection<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithMySqlConnection<'_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithMySqlConnection<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithMySqlConnection<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithMySqlConnection<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithMySqlConnection<'_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * REDIS CONNECTION IMPL
 **************************************/

pub struct WithRedisConnection<'a, E> {
    redis_conn: &'a mut RedisConnection,
    extra: E,
}

impl<E: Ctx> HasRedisConnection for WithRedisConnection<'_, E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        self.redis_conn
    }
}

impl<E: Ctx> Ctx for WithRedisConnection<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithRedisConnection<'_, E> {
    #[inline]
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<E: HasMap> HasMap for WithRedisConnection<'_, E> {
    #[inline]
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithRedisConnection<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithRedisConnection<'_, E> {
    #[inline]
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithRedisConnection<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithRedisConnection<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithRedisConnection<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithRedisConnection<'_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithRedisConnection<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithRedisConnection<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithRedisConnection<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithRedisConnection<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithRedisConnection<'_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * DB CONNECTION IMPL
 **************************************/

pub struct WithDbConnection<'a, 'b, E> {
    conn: &'a mut DatabaseConnection<'b>,
    extra: E,
}

impl<'a, E: Ctx> HasDbConnection<'a> for WithDbConnection<'_, 'a, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        self.conn
    }
}

impl<E: Ctx> HasMySqlConnection for WithDbConnection<'_, '_, E> {
    #[inline]
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        &mut self.conn.mysql_conn
    }
}

impl<E: Ctx> HasRedisConnection for WithDbConnection<'_, '_, E> {
    #[inline]
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        &mut self.conn.redis_conn
    }
}

impl<E: Ctx> Ctx for WithDbConnection<'_, '_, E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithDbConnection<'_, '_, E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithDbConnection<'_, '_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithDbConnection<'_, '_, E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithDbConnection<'_, '_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithDbConnection<'_, '_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithDbConnection<'_, '_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithDbConnection<'_, '_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithDbConnection<'_, '_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithDbConnection<'_, '_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithDbConnection<'_, '_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithDbConnection<'_, '_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * EVENT IMPL
 **************************************/

pub struct WithEvent<'a, E> {
    event: &'a models::Event,
    extra: E,
}

impl<E: Ctx> HasEvent for WithEvent<'_, E> {
    fn get_event(&self) -> &models::Event {
        self.event
    }
}

impl<E: Ctx> HasEventId for WithEvent<'_, E> {
    fn get_event_id(&self) -> u32 {
        self.event.id
    }
}

impl<E: Ctx> Ctx for WithEvent<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithEvent<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithEvent<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithEvent<'_, E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithEvent<'_, E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithEvent<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithEvent<'_, E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithEvent<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithEvent<'_, E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithEvent<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithEvent<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithEvent<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithEvent<'_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * EVENT ID IMPL
 **************************************/

pub struct WithEventId<E> {
    event_id: u32,
    extra: E,
}

impl<E: Ctx> HasEventId for WithEventId<E> {
    fn get_event_id(&self) -> u32 {
        self.event_id
    }
}

impl<E: Ctx> Ctx for WithEventId<E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithEventId<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithEventId<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithEventId<E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithEventId<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithEventId<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithEventId<E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasMap> HasMap for WithEventId<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithEventId<E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithEventId<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithEventId<E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithEventId<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithEventId<E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithEventId<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * EVENT EDITION IMPL
 **************************************/

pub struct WithEdition<'a, E> {
    edition: &'a models::EventEdition,
    extra: E,
}

impl<E: Ctx> HasEdition for WithEdition<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        self.edition
    }
}

impl<E: Ctx> HasEventId for WithEdition<'_, E> {
    fn get_event_id(&self) -> u32 {
        self.edition.event_id
    }
}

impl<E: Ctx> HasEditionId for WithEdition<'_, E> {
    fn get_edition_id(&self) -> u32 {
        self.edition.id
    }
}

impl<E: Ctx> Ctx for WithEdition<'_, E> {
    #[inline]
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithEdition<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithEdition<'_, E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithEdition<'_, E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithEdition<'_, E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithEdition<'_, E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithEdition<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithEdition<'_, E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithEdition<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithEdition<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithEdition<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithEdition<'_, E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * EVENT EDITION ID IMPL
 **************************************/

pub struct WithEditionId<E> {
    edition_id: u32,
    extra: E,
}

impl<E: Ctx> HasEditionId for WithEditionId<E> {
    fn get_edition_id(&self) -> u32 {
        self.edition_id
    }
}

impl<E: Ctx> Ctx for WithEditionId<E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithEditionId<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithEditionId<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<'a, E: HasDbConnection<'a>> HasDbConnection<'a> for WithEditionId<E> {
    fn get_db_conn(&mut self) -> &mut DatabaseConnection<'a> {
        <E as HasDbConnection<'a>>::get_db_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithEditionId<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithEditionId<E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithEditionId<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithEditionId<E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithEditionId<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithEditionId<E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithEditionId<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithEditionId<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithEditionId<E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithEditionId<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * REDIS POOL IMPL
 **************************************/

pub struct WithRedisPool<E> {
    pool: RedisPool,
    extra: E,
}

impl<E: Ctx> HasRedisPool for WithRedisPool<E> {
    fn get_redis_pool(&self) -> RedisPool {
        self.pool.clone()
    }
}

impl<E: Ctx> Ctx for WithRedisPool<E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithRedisPool<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithRedisPool<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithRedisPool<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithRedisPool<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithRedisPool<E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithRedisPool<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithRedisPool<E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithRedisPool<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithRedisPool<E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithRedisPool<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithRedisPool<E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithRedisPool<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * MYSQL POOL IMPL
 **************************************/

pub struct WithMySqlPool<E> {
    pool: MySqlPool,
    extra: E,
}

impl<E: Ctx> HasMySqlPool for WithMySqlPool<E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        self.pool.clone()
    }
}

impl<E: Ctx> Ctx for WithMySqlPool<E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithMySqlPool<E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithMySqlPool<E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithMySqlPool<E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithMySqlPool<E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithMySqlPool<E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithMySqlPool<E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithMySqlPool<E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithMySqlPool<E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithMySqlPool<E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithMySqlPool<E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithMySqlPool<E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}

impl<E: HasPlayerLogin> HasPlayerLogin for WithMySqlPool<E> {
    fn get_player_login(&self) -> &str {
        <E as HasPlayerLogin>::get_player_login(&self.extra)
    }
}

/**************************************
 * PLAYER LOGIN IMPL
 **************************************/

pub struct WithPlayerLogin<'a, E> {
    login: &'a str,
    extra: E,
}

impl<E: Ctx> HasPlayerLogin for WithPlayerLogin<'_, E> {
    fn get_player_login(&self) -> &str {
        self.login
    }
}

impl<E: Ctx> Ctx for WithPlayerLogin<'_, E> {
    fn ctx(&self) -> &Context<'_> {
        <E as Ctx>::ctx(&self.extra)
    }
}

impl<E: HasRedisPool> HasRedisPool for WithPlayerLogin<'_, E> {
    fn get_redis_pool(&self) -> RedisPool {
        <E as HasRedisPool>::get_redis_pool(&self.extra)
    }
}

impl<E: HasMySqlPool> HasMySqlPool for WithPlayerLogin<'_, E> {
    fn get_mysql_pool(&self) -> MySqlPool {
        <E as HasMySqlPool>::get_mysql_pool(&self.extra)
    }
}

impl<E: HasRedisConnection> HasRedisConnection for WithPlayerLogin<'_, E> {
    fn get_redis_conn(&mut self) -> &mut RedisConnection {
        <E as HasRedisConnection>::get_redis_conn(&mut self.extra)
    }
}

impl<E: HasMySqlConnection> HasMySqlConnection for WithPlayerLogin<'_, E> {
    fn get_mysql_conn(&mut self) -> &mut sqlx::MySqlConnection {
        <E as HasMySqlConnection>::get_mysql_conn(&mut self.extra)
    }
}

impl<E: HasPlayer> HasPlayer for WithPlayerLogin<'_, E> {
    fn get_player(&self) -> &models::Player {
        <E as HasPlayer>::get_player(&self.extra)
    }
}

impl<E: HasPlayerId> HasPlayerId for WithPlayerLogin<'_, E> {
    fn get_player_id(&self) -> u32 {
        <E as HasPlayerId>::get_player_id(&self.extra)
    }
}

impl<E: HasMap> HasMap for WithPlayerLogin<'_, E> {
    fn get_map(&self) -> &models::Map {
        <E as HasMap>::get_map(&self.extra)
    }
}

impl<E: HasMapId> HasMapId for WithPlayerLogin<'_, E> {
    fn get_map_id(&self) -> u32 {
        <E as HasMapId>::get_map_id(&self.extra)
    }
}

impl<E: HasEvent> HasEvent for WithPlayerLogin<'_, E> {
    fn get_event(&self) -> &models::Event {
        <E as HasEvent>::get_event(&self.extra)
    }
}

impl<E: HasEventId> HasEventId for WithPlayerLogin<'_, E> {
    fn get_event_id(&self) -> u32 {
        <E as HasEventId>::get_event_id(&self.extra)
    }
}

impl<E: HasEdition> HasEdition for WithPlayerLogin<'_, E> {
    fn get_edition(&self) -> &models::EventEdition {
        <E as HasEdition>::get_edition(&self.extra)
    }
}

impl<E: HasEditionId> HasEditionId for WithPlayerLogin<'_, E> {
    fn get_edition_id(&self) -> u32 {
        <E as HasEditionId>::get_edition_id(&self.extra)
    }
}
