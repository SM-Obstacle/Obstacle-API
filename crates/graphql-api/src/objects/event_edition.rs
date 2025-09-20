use std::borrow::Cow;

use deadpool_redis::redis::AsyncCommands as _;
use entity::{event, event_category, event_edition, event_edition_categories};
use futures::TryStreamExt as _;
use records_lib::{
    Expirable as _, RedisPool,
    error::RecordsResult,
    event::{self as event_utils, EventMap},
    internal,
    mappack::AnyMappackId,
    must,
    redis_key::mappack_time_key,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _, QueryFilter as _, sea_query::Query,
};

use crate::objects::{
    event::Event, event_category::EventCategory, event_edition_map::EventEditionMap,
    event_edition_player::EventEditionPlayer, mappack::Mappack, player::Player,
};

#[derive(Debug, Clone)]
pub struct EventEdition<'a> {
    pub(super) event: Cow<'a, Event>,
    pub(super) inner: event_edition::Model,
}

impl EventEdition<'_> {
    pub(super) async fn from_inner<C: ConnectionTrait>(
        conn: &C,
        inner: event_edition::Model,
    ) -> RecordsResult<Self> {
        let event = event::Entity::find_by_id(inner.event_id)
            .into_model()
            .one(conn)
            .await?
            .ok_or_else(|| internal!("Event {} should be in database", inner.event_id))?;
        Ok(Self {
            event: Cow::Owned(event),
            inner,
        })
    }
}

#[async_graphql::Object]
impl EventEdition<'_> {
    async fn id(&self) -> u32 {
        self.inner.id
    }

    async fn mappack(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<Mappack>> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mut redis_conn = redis_pool.get().await?;

        let mappack_id = AnyMappackId::Event(&self.event.inner, &self.inner);
        let last_update_time: Option<i64> = redis_conn.get(mappack_time_key(mappack_id)).await?;

        Ok(last_update_time.map(|_| Mappack {
            mappack_id: mappack_id.mappack_id().to_string(),
            event_has_expired: self.inner.has_expired(),
        }))
    }

    async fn admins(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Player>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let a = event_utils::get_admins_of(conn, self.inner.event_id, self.inner.id)
            .await?
            .map_ok(From::from)
            .try_collect()
            .await?;
        Ok(a)
    }

    async fn event(&self) -> &Event {
        &self.event
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn subtitle(&self) -> Option<&str> {
        self.inner.subtitle.as_deref()
    }

    async fn start_date(&self) -> &chrono::NaiveDateTime {
        &self.inner.start_date
    }

    async fn banner_img_url(&self) -> Option<&str> {
        self.inner.banner_img_url.as_deref()
    }

    async fn expires_in(&self) -> Option<i64> {
        self.inner.expires_in()
    }

    async fn player(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<EventEditionPlayer<'_>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let player = must::have_player(conn, &login).await?;
        Ok(EventEditionPlayer {
            edition: self,
            player,
        })
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
        game_id: String,
    ) -> async_graphql::Result<EventEditionMap<'_>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let EventMap { map, .. } =
            event_utils::get_map_in_edition(conn, &game_id, self.inner.event_id, self.inner.id)
                .await?
                .ok_or_else(|| async_graphql::Error::new("Map not found in this edition"))?;

        Ok(EventEditionMap {
            edition: self,
            map: map.into(),
        })
    }

    async fn categories(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventCategory>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let q = event_category::Entity::find()
            .filter(
                event_category::Column::Id.in_subquery(
                    Query::select()
                        .from(event_edition_categories::Entity)
                        .and_where(
                            event_edition_categories::Column::EventId
                                .eq(self.inner.event_id)
                                .and(event_edition_categories::Column::EditionId.eq(self.inner.id)),
                        )
                        .column(event_edition_categories::Column::CategoryId)
                        .take(),
                ),
            )
            .into_model()
            .all(conn)
            .await?;
        Ok(q)
    }
}
