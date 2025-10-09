use std::borrow::Cow;

use entity::{
    event, event_admins, event_categories, event_category, event_edition, event_edition_maps,
    players,
};
use records_lib::event as event_utils;
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, FromQueryResult, QueryFilter as _, QueryOrder as _,
    QuerySelect as _,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func, Query},
};

use crate::{
    error::GqlResult,
    objects::{event_category::EventCategory, event_edition::EventEdition, player::Player},
};

#[derive(Debug, Clone, FromQueryResult)]
pub struct Event {
    #[sea_orm(nested)]
    pub inner: event::Model,
}

impl From<event::Model> for Event {
    fn from(inner: event::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Event {
    async fn handle(&self) -> &str {
        &self.inner.handle
    }

    async fn cooldown(&self) -> Option<u32> {
        self.inner.cooldown
    }

    async fn admins(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<Player>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let q = players::Entity::find()
            .filter(
                players::Column::Id.in_subquery(
                    Query::select()
                        .from(event_admins::Entity)
                        .and_where(event_admins::Column::EventId.eq(self.inner.id))
                        .column(event_admins::Column::PlayerId)
                        .take(),
                ),
            )
            .into_model()
            .all(conn)
            .await?;

        Ok(q)
    }

    async fn categories(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<EventCategory>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let q = event_category::Entity::find()
            .filter(
                event_category::Column::Id.in_subquery(
                    Query::select()
                        .from(event_categories::Entity)
                        .and_where(event_categories::Column::EventId.eq(self.inner.id))
                        .column(event_categories::Column::CategoryId)
                        .take(),
                ),
            )
            .into_model()
            .all(conn)
            .await?;

        Ok(q)
    }

    async fn editions(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let q = event_utils::event_editions_list(conn, &self.inner.handle).await?;

        Ok(q.into_iter()
            .map(|inner| EventEdition {
                event: Cow::Borrowed(self),
                inner,
            })
            .collect())
    }

    async fn last_edition(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_edition::Entity::find()
            .reverse_join(event_edition_maps::Entity)
            .filter(
                event_edition::Column::EventId
                    .eq(self.inner.id)
                    .and(Expr::col(event_edition::Column::StartDate).lt(Func::cust("SYSDATE")))
                    .and(
                        event_edition::Column::Ttl
                            .is_null()
                            .or(Func::cust("TIMESTAMPADD")
                                .arg(Expr::custom_keyword("SECOND"))
                                .arg(Expr::col(event_edition::Column::Ttl))
                                .arg(Expr::col(event_edition::Column::StartDate))
                                .gt(Func::cust("SYSDATE"))),
                    ),
            )
            .order_by_desc(event_edition::Column::Id)
            .limit(1)
            .one(conn)
            .await?;

        Ok(edition.map(|inner| EventEdition {
            event: Cow::Borrowed(self),
            inner,
        }))
    }

    async fn edition(
        &self,
        ctx: &async_graphql::Context<'_>,
        edition_id: u32,
    ) -> GqlResult<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_utils::get_edition_by_id(conn, self.inner.id, edition_id).await?;

        Ok(edition.map(|inner| EventEdition {
            event: Cow::Borrowed(self),
            inner,
        }))
    }
}
