use entity::{event_category, event_edition_maps, maps, players, records};
use futures::{Stream as _, StreamExt as _};
use records_lib::{event as event_utils, mappack::AnyMappackId};
use sea_orm::{
    ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _, QuerySelect as _,
    RelationTrait as _, prelude::Expr, sea_query::IntoCondition as _,
};

use crate::objects::{
    event_edition::EventEdition, event_edition_map_ext::EventEditionMapExt,
    event_edition_player_categorized_rank::EventEditionPlayerCategorizedRank, mappack,
    player::Player,
};

pub struct EventEditionPlayer<'a> {
    pub edition: &'a EventEdition<'a>,
    pub player: players::Model,
}

#[async_graphql::Object]
impl EventEditionPlayer<'_> {
    async fn player(&self) -> Player {
        self.player.clone().into()
    }

    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        mappack::player_rank(
            ctx,
            AnyMappackId::Event(&self.edition.event.inner, &self.edition.inner),
            self.player.id,
        )
        .await
    }

    async fn rank_avg(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<f64> {
        mappack::player_rank_avg(
            ctx,
            AnyMappackId::Event(&self.edition.event.inner, &self.edition.inner),
            self.player.id,
        )
        .await
    }

    async fn map_finished(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        mappack::player_map_finished(
            ctx,
            AnyMappackId::Event(&self.edition.event.inner, &self.edition.inner),
            self.player.id,
        )
        .await
    }

    async fn worst_rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i32> {
        mappack::player_worst_rank(
            ctx,
            AnyMappackId::Event(&self.edition.event.inner, &self.edition.inner),
            self.player.id,
        )
        .await
    }

    async fn categorized_ranks(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventEditionPlayerCategorizedRank<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let categories = event_utils::get_categories_by_edition_id(
            conn,
            self.edition.inner.event_id,
            self.edition.inner.id,
        )
        .await?;

        let categories = if categories.is_empty() {
            vec![event_category::Model {
                name: "All maps".to_owned(),
                ..Default::default()
            }]
        } else {
            categories
        };

        Ok(categories
            .into_iter()
            .map(|category| EventEditionPlayerCategorizedRank {
                category,
                player: self,
            })
            .collect())
    }

    async fn unfinished_maps(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventEditionMapExt<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let player_id = self.player.id;

        let mut unfinished_maps = maps::Entity::find()
            .join_rev(
                sea_orm::JoinType::InnerJoin,
                event_edition_maps::Relation::Maps.def(),
            )
            .join(
                sea_orm::JoinType::LeftJoin,
                maps::Relation::Records
                    .def()
                    .on_condition(move |_left, right| {
                        Expr::col((right, records::Column::RecordPlayerId))
                            .eq(player_id)
                            .into_condition()
                    })
                    .condition_type(sea_orm::sea_query::ConditionType::All),
            )
            .join(
                sea_orm::JoinType::LeftJoin,
                records::Relation::EventEditionRecords.def(),
            )
            .filter(
                event_edition_maps::Column::EventId
                    .eq(self.edition.inner.event_id)
                    .and(event_edition_maps::Column::EditionId.eq(self.edition.inner.id))
                    .and(records::Column::RecordId.is_null()),
            )
            .stream(conn)
            .await?;

        let mut out = Vec::with_capacity(unfinished_maps.size_hint().0);

        while let Some(map) = unfinished_maps.next().await {
            out.push(EventEditionMapExt {
                edition_player: self,
                inner: From::<maps::Model>::from(map?),
            });
        }

        Ok(out)
    }
}
