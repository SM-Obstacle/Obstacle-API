//! This module contains all the Redis keys constructors.
//!
//! Redis is a key-value database. This module contains all the keys constructors for every kind of
//! value we might want during a computation.
//!
//! For example, we store the ranks of the players in the Redis database as ZSETs. The key
//! to the ZSET of a map with an ID `X` is `v3:lb:X`. To get it, we must use the [`alone_map_key`]
//! function by providing the map ID.

use core::fmt;

use deadpool_redis::redis::{RedisWrite, ToRedisArgs};

use crate::{event::OptEvent, mappack::AnyMappackId};

const V3_KEY_PREFIX: &str = "v3";

const V3_MAPPACK_KEY_PREFIX: &str = "mappack";

const V3_MAP_KEY_PREFIX: &str = "lb";

const V3_EVENT_KEY_PREFIX: &str = "event";

const V3_TOKEN_KEY_PREFIX: &str = "token";
const V3_TOKEN_WEB_KEY_PREFIX: &str = "web";
const V3_TOKEN_MP_KEY_PREFIX: &str = "mp";

const V3_MAPPACK_TIME: &str = "time";
const V3_MAPPACK_NB_MAP: &str = "nb_map";
const V3_MAPPACK_MX_USERNAME: &str = "mx_username";
const V3_MAPPACK_MX_NAME: &str = "mx_name";
const V3_MAPPACK_MX_CREATED: &str = "mx_created";
const V3_MAPPACK_LB: &str = "lb";
const V3_MAPPACK_LB_RANK_AVG: &str = "rank_avg";
const V3_MAPPACK_LB_MAP_FINISHED: &str = "map_finished";
const V3_MAPPACK_LB_WORST_RANK: &str = "worst_rank";
const V3_MAPPACK_LB_RANKS: &str = "ranks";

const V3_MAPPACK_MAP_KEY_PREFIX: &str = "map";
const V3_MAPPACK_MAP_LAST_RANK: &str = "last_rank";

const V3_CACHED: &str = "cached";

macro_rules! create_key {
    (
        $(#[$($attr:tt)*])*
        struct $name:ident$(<$l:lifetime => $_:lifetime>)? = $fn_name:ident $({
            $(
                $(#[$($field_attr:tt)*])*
                $field:ident: $field_ty:ty
            ),* $(,)?
        })?$(;$semicolon:tt)?
        |$self:ident, $f:ident| $fmt_expr:expr
    ) => {
        #[doc = concat!("The `", stringify!($name), "` Redis key.")]
        $(#[$($attr)*])*
        #[derive(Debug)]
        pub struct $name$(<$l>)? $({
            $(
                $(#[$($field_attr)*])*
                pub $field: $field_ty
            ),*
        })?$($semicolon)?

        #[doc = concat!("The constructor of the `", stringify!($name), "` Redis key.")]
        #[inline(always)]
        pub fn $fn_name$(<$l>)?($($($field: $field_ty),*)?) -> $name$(<$l>)? {
            $name { $($($field),*)? }
        }

        impl ToRedisArgs for $name$(<$_>)? {
            #[inline(always)]
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + RedisWrite,
            {
                out.write_arg_fmt(self);
            }
        }

        impl fmt::Display for $name$(<$_>)? {
            fn fmt(&$self, $f: &mut fmt::Formatter<'_>) -> fmt::Result {
                $fmt_expr
            }
        }
    }
}

create_key! {
    ///
    /// The cached key is a key that was copied from another.
    struct CachedKey = cached_key {
        /// The cached key name.
        key_name: String,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_CACHED}:{}", self.key_name)
}

create_key! {
    struct MappacksKey = mappacks_key;;
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}")
}

create_key! {
    ///
    /// The mappack key returns a Redis SET containing UIDs of the maps of the mappack.
    struct MappackKey<'a => '_> = mappack_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// The map key (or alone map key, as it isn't bound to an event) returns a ZSET containing
    /// the IDs of the players who finished the map sorted by their times on it.
    ///
    /// For its event version, see [`event_map_key`].
    struct AloneMapKey = alone_map_key {
        /// The ID of the map.
        map_id: u32,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAP_KEY_PREFIX}:{}", self.map_id)
}

/// The `EventMapKey` Redis key.
#[derive(Debug)]
pub struct EventMapKey<'a> {
    /// The Redis key to the leaderboard of the map.
    pub map_key: AloneMapKey,
    /// The event handle.
    pub event_handle: &'a str,
    /// The edition ID.
    pub edition_id: u32,
}

/// The constructor of the `EventMapKey` Redis key.
#[inline(always)]
pub fn event_map_key(map_id: u32, event_handle: &str, edition_id: u32) -> EventMapKey<'_> {
    EventMapKey {
        map_key: alone_map_key(map_id),
        event_handle,
        edition_id,
    }
}

impl ToRedisArgs for EventMapKey<'_> {
    #[inline(always)]
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg_fmt(self)
    }
}

impl fmt::Display for EventMapKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{V3_KEY_PREFIX}:{V3_EVENT_KEY_PREFIX}:{}:{}:{}",
            self.event_handle, self.edition_id, self.map_key
        )
    }
}

/// The `MapKey` Redis key.
///
/// This is a generic version of the [`AloneMapKey`] and [`EventMapKey`] structs.
pub enum MapKey<'a> {
    /// The key isn't bound to an event.
    Alone(AloneMapKey),
    /// The key is bound to an event.
    Evented(EventMapKey<'a>),
}

impl ToRedisArgs for MapKey<'_> {
    #[inline(always)]
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg_fmt(self)
    }
}

impl fmt::Display for MapKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapKey::Alone(a) => a.fmt(f),
            MapKey::Evented(b) => b.fmt(f),
        }
    }
}

/// The constructor of the `MapKey` Redis key.
///
/// This is a generic version of the [`alone_map_key`] and [`event_map_key`] functions.
pub fn map_key<'a>(map_id: u32, event: OptEvent<'a, 'a>) -> MapKey<'a> {
    match event {
        Some((event, edition)) => MapKey::Evented(event_map_key(map_id, &event.handle, edition.id)),
        None => MapKey::Alone(alone_map_key(map_id)),
    }
}

/// Tiny module used to help the specialization of the [`TokenKey`] Redis key.
pub mod token_kind {
    /// The type of the constants.
    pub type Ty = bool;

    /// The token kind for the website.
    pub const WEB: Ty = true;
    /// The token kind for the gamemode.
    pub const MP: Ty = false;
}

/// The `TokenKey` Redis key.
///
/// This key is used to store the hashes of the authentication tokens of the players.
pub struct TokenKey<'a, const KIND: token_kind::Ty> {
    /// The login of the player.
    pub login: &'a str,
}

/// The constructor of the `TokenKey` Redis key in its "website" version.
#[inline(always)]
pub fn web_token_key(login: &str) -> TokenKey<'_, { token_kind::WEB }> {
    TokenKey { login }
}

/// The constructor of the `TokenKey` Redis key in its "gamemode" version.
#[inline(always)]
pub fn mp_token_key(login: &str) -> TokenKey<'_, { token_kind::MP }> {
    TokenKey { login }
}

impl<const KIND: token_kind::Ty> ToRedisArgs for TokenKey<'_, KIND>
where
    Self: fmt::Display,
{
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg_fmt(self);
    }
}

impl fmt::Display for TokenKey<'_, { token_kind::WEB }> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{V3_KEY_PREFIX}:{V3_TOKEN_KEY_PREFIX}:{V3_TOKEN_WEB_KEY_PREFIX}:{}",
            self.login
        )
    }
}

impl fmt::Display for TokenKey<'_, { token_kind::MP }> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{V3_KEY_PREFIX}:{V3_TOKEN_KEY_PREFIX}:{V3_TOKEN_MP_KEY_PREFIX}:{}",
            self.login
        )
    }
}

create_key! {
    ///
    /// This key points to the timestamp of the last time the mappack content was updated.
    struct MappackTimeKey<'a => '_> = mappack_time_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_TIME}", self.mappack.mappack_id())
}

create_key! {
    ///
    /// This key points to the amount of maps the mappack contains.
    struct MappackNbMapKey<'a => '_> = mappack_nb_map_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_NB_MAP}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// This key points to the username of the author of the MX mappack. It is not used if the mappack
    /// is bound to an event.
    struct MappackMxUsernameKey<'a => '_> = mappack_mx_username_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_MX_USERNAME}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// This key points to the name of the MX mappack. It is not used if the mappack is bound to
    /// an event.
    struct MappackMxNameKey<'a => '_> = mappack_mx_name_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_MX_NAME}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// This key points to the date of creation of the MX mappack. It is not used if the mappack
    /// is bound to an event.
    struct MappackMxCreatedKey<'a => '_> = mappack_mx_created_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(f, "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_MX_CREATED}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// This key points to a ZSET containing the IDs of the players sorted by their score.
    struct MappackLbKey<'a => '_> = mappack_lb_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_LB}",
        self.mappack.mappack_id()
    )
}

create_key! {
    ///
    /// This key points to the rank average of the provided player in the provided mappack.
    struct MappackPlayerRankAvg<'a => '_> = mappack_player_rank_avg_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
        /// The player ID.
        player_id: u32,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_LB}:{}:{V3_MAPPACK_LB_RANK_AVG}",
        self.mappack.mappack_id(), self.player_id
    )
}

create_key! {
    ///
    /// This key points to the amount of finished maps of the provided player
    /// in the provided mappack.
    struct MappackPlayerMapFinished<'a => '_> = mappack_player_map_finished_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
        /// The player ID.
        player_id: u32,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_LB}:{}:{V3_MAPPACK_LB_MAP_FINISHED}",
        self.mappack.mappack_id(), self.player_id
    )
}

create_key! {
    ///
    /// This key points to the worst rank of the provided player on the provided mappack.
    struct MappackPlayerWorstRank<'a => '_> = mappack_player_worst_rank_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
        /// The player ID.
        player_id: u32,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_LB}:{}:{V3_MAPPACK_LB_WORST_RANK}",
        self.mappack.mappack_id(), self.player_id
    )
}

create_key! {
    ///
    /// This key points to a ZSET containing the maps UIDs the player has finished
    /// in the provided mappack.
    struct MappackPlayerRanks<'a => '_> = mappack_player_ranks_key {
        /// The mappack.
        mappack: AnyMappackId<'a>,
        /// The player ID.
        player_id: u32,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_LB}:{}:{V3_MAPPACK_LB_RANKS}",
        self.mappack.mappack_id(), self.player_id
    )
}

create_key! {
    ///
    /// This key points to the last rank of the provided map on the provided mappack.
    struct MappackMapLastRank<'a => '_> = mappack_map_last_rank {
        /// The mappack.
        mappack: AnyMappackId<'a>,
        /// The map UID.
        map_uid: &'a str,
    }
    |self, f| write!(
        f,
        "{V3_KEY_PREFIX}:{V3_MAPPACK_KEY_PREFIX}:{}:{V3_MAPPACK_MAP_KEY_PREFIX}:{}:{V3_MAPPACK_MAP_LAST_RANK}",
        self.mappack.mappack_id(), self.map_uid
    )
}
