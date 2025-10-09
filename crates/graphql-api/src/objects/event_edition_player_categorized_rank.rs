use entity::{
    event_category, event_edition, event_edition_maps, global_event_records, global_records, maps,
    records,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait as _, DbConn, EntityTrait as _, PaginatorTrait as _,
    QueryFilter as _, QuerySelect as _, prelude::Expr, sea_query::Query,
};

use crate::{
    error::GqlResult,
    objects::{
        event_edition_player::EventEditionPlayer, event_edition_player_rank::EventEditionPlayerRank,
    },
};

pub struct EventEditionPlayerCategorizedRank<'a> {
    pub player: &'a EventEditionPlayer<'a>,
    pub category: event_category::Model,
}

#[async_graphql::Object]
impl EventEditionPlayerCategorizedRank<'_> {
    async fn category_name(&self) -> &str {
        &self.category.name
    }

    async fn banner_img_url(&self) -> Option<&str> {
        self.category.banner_img_url.as_deref()
    }

    async fn hex_color(&self) -> Option<&str> {
        self.category.hex_color.as_deref()
    }

    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<u64> {
        let conn = ctx.data_unchecked::<DbConn>();
        let edition = &self.player.edition.inner;

        let n = event_edition_maps::Entity::find()
            .inner_join(event_edition::Entity)
            .join(
                sea_orm::JoinType::InnerJoin,
                event_edition_maps::Entity::belongs_to(event_category::Entity)
                    .from(event_edition_maps::Column::CategoryId)
                    .to(event_category::Column::Id)
                    .into(),
            )
            .filter(
                event_edition_maps::Column::EventId
                    .eq(edition.event_id)
                    .and(event_edition_maps::Column::EditionId.eq(edition.id))
                    .and(event_category::Column::Id.eq(self.category.id)),
            )
            .count(conn)
            .await?;

        Ok(n)
    }

    async fn ranks(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<Vec<EventEditionPlayerRank<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let mut select = Query::select();
        let select = select
            .join(
                sea_orm::JoinType::InnerJoin,
                maps::Entity,
                Expr::col((maps::Entity, maps::Column::Id))
                    .eq(Expr::col(("r", records::Column::MapId))),
            )
            .join(
                sea_orm::JoinType::InnerJoin,
                event_edition_maps::Entity,
                Expr::col((maps::Entity, maps::Column::Id)).eq(Expr::col((
                    event_edition_maps::Entity,
                    event_edition_maps::Column::MapId,
                ))),
            )
            .and_where(
                event_edition_maps::Column::EventId
                    .eq(self.player.edition.inner.event_id)
                    .and(event_edition_maps::Column::EditionId.eq(self.player.edition.inner.id))
                    .and(
                        Expr::col(("r", records::Column::RecordPlayerId)).eq(self.player.player.id),
                    ),
            )
            .order_by(event_edition_maps::Column::Order, sea_orm::Order::Asc)
            .column(maps::Column::GameId)
            .expr(Expr::col(("r", records::Column::Time)));

        if self.category.id != 0 {
            select
                .join(
                    sea_orm::JoinType::InnerJoin,
                    event_category::Entity,
                    Expr::col((event_category::Entity, event_category::Column::Id))
                        .eq(Expr::col(event_edition_maps::Column::CategoryId)),
                )
                .and_where(
                    Expr::col((event_category::Entity, event_category::Column::Id))
                        .eq(self.category.id),
                );
        }

        if self.player.edition.inner.is_transparent != 0 {
            select.from_as(global_records::Entity, "r");
        } else {
            select.from_as(global_event_records::Entity, "r");
        }

        let stmt = conn.get_database_backend().build(&*select);
        let res = conn
            .query_all(stmt)
            .await?
            .into_iter()
            .map(|result| {
                let game_id = result.try_get("", "game_id").unwrap();
                let time = result.try_get("", "time").unwrap();
                EventEditionPlayerRank {
                    map_game_id: game_id,
                    record_time: time,
                    edition_player: self.player,
                }
            })
            .collect();

        Ok(res)
    }
}
