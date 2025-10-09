use async_graphql::dataloader::DataLoader;
use entity::event_edition_maps;
use records_lib::{event as event_utils, internal, opt_event::OptEvent};
use sea_orm::{DbConn, EntityTrait as _, QuerySelect as _};

use crate::{
    error::GqlResult,
    loaders::map::MapLoader,
    objects::{
        event_edition::EventEdition, map::Map, medal_times::MedalTimes,
        ranked_record::RankedRecord, sort_state::SortState,
    },
};

#[derive(async_graphql::SimpleObject)]
#[graphql(complex)]
pub struct EventEditionMap<'a> {
    pub edition: &'a EventEdition<'a>,
    pub map: Map,
}

#[async_graphql::ComplexObject]
impl EventEditionMap<'_> {
    async fn link_to_original(&self) -> bool {
        !(self.edition.inner.save_non_event_record != 0
            && self.edition.inner.non_original_maps != 0)
            && self.edition.inner.is_transparent == 0
    }

    async fn original_map(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<Map>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let original_map_id = entity::event_edition_maps::Entity::find_by_id((
            self.edition.inner.event_id,
            self.edition.inner.id,
            self.map.inner.id,
        ))
        .select_only()
        .column(event_edition_maps::Column::OriginalMapId)
        .into_tuple::<Option<_>>()
        .one(conn)
        .await?
        .ok_or_else(|| {
            internal!(
                "event_edition_maps({}, {}, {}) must exist in database",
                self.edition.inner.event_id,
                self.edition.inner.id,
                self.map.inner.id
            )
        })?;

        let map = match original_map_id {
            Some(id) => Some(
                map_loader
                    .load_one(id)
                    .await?
                    .ok_or_else(|| internal!("unknown original_map_id: {id}"))?,
            ),
            _ => None,
        };

        Ok(map)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> GqlResult<Vec<RankedRecord>> {
        self.map
            .get_records(
                ctx,
                OptEvent::new(&self.edition.event.inner, &self.edition.inner),
                rank_sort_by,
                date_sort_by,
            )
            .await
    }

    async fn medal_times(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<MedalTimes>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let medal_times = event_utils::get_medal_times_of(
            conn,
            self.edition.inner.event_id,
            self.edition.inner.id,
            self.map.inner.id,
        )
        .await?;

        Ok(medal_times.map(From::from))
    }
}
