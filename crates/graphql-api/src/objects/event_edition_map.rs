use async_graphql::{ID, connection, dataloader::DataLoader};
use entity::{event_edition_maps, global_event_records};
use records_lib::{event as event_utils, internal, opt_event::OptEvent};
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait as _, PaginatorTrait as _, QueryFilter as _};

use crate::{
    error::GqlResult,
    loaders::{event_edition_map::EventEditionMapLoader, map::MapLoader},
    objects::{
        event_edition::EventEdition, map::Map, medal_times::MedalTimes,
        ranked_record::RankedRecord, records_filter::RecordsFilter, sort::MapRecordSort,
        sort_state::SortState,
    },
};

#[derive(async_graphql::SimpleObject)]
#[graphql(complex)]
pub struct EventEditionMap<'a> {
    pub edition: &'a EventEdition<'a>,
    pub map: Map,
}

impl EventEditionMap<'_> {
    /// Loads the row binding this map to its event edition.
    ///
    /// Several fields are read off that single row, so they all go through the loader rather
    /// than querying it once each.
    async fn load_row(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<event_edition_maps::Model> {
        let key = (
            self.edition.inner.event_id,
            self.edition.inner.id,
            self.map.inner.id,
        );

        let row = ctx
            .data_unchecked::<DataLoader<EventEditionMapLoader>>()
            .load_one(key)
            .await?
            .ok_or_else(|| {
                internal!(
                    "event_edition_maps({}, {}, {}) must exist in database",
                    key.0,
                    key.1,
                    key.2
                )
            })?;

        Ok(row)
    }
}

pub(crate) async fn get_event_edition_map_records_amount<C: sea_orm::ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
    map_id: u32,
) -> GqlResult<usize> {
    let amount = global_event_records::Entity::find()
        .filter(global_event_records::Column::MapId.eq(map_id))
        .filter(global_event_records::Column::EventId.eq(event_id))
        .filter(global_event_records::Column::EditionId.eq(edition_id))
        .count(conn)
        .await?;
    Ok(amount as usize)
}

#[async_graphql::ComplexObject]
impl EventEditionMap<'_> {
    async fn records_amount(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        let conn = ctx.data_unchecked::<DbConn>();
        get_event_edition_map_records_amount(
            conn,
            self.edition.inner.event_id,
            self.edition.inner.id,
            self.map.inner.id,
        )
        .await
    }

    async fn link_to_original(&self) -> bool {
        !(self.edition.inner.save_non_event_record != 0
            && self.edition.inner.non_original_maps != 0)
            && self.edition.inner.is_transparent == 0
    }

    async fn original_map(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<Map>> {
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let map = match self.load_row(ctx).await?.original_map_id {
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

    #[allow(clippy::too_many_arguments)]
    async fn records_connection(
        &self,
        ctx: &async_graphql::Context<'_>,
        #[graphql(desc = "Cursor to fetch records after (for forward pagination)")] after: Option<
            String,
        >,
        #[graphql(desc = "Cursor to fetch records before (for backward pagination)")]
        before: Option<String>,
        #[graphql(desc = "Number of records to fetch (default: 50, max: 100)")] first: Option<i32>,
        #[graphql(desc = "Number of records to fetch from the end (for backward pagination)")] last: Option<i32>,
        sort: Option<MapRecordSort>,
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        self.map
            .get_records_connection(
                ctx,
                OptEvent::new(&self.edition.event.inner, &self.edition.inner),
                after,
                before,
                first,
                last,
                sort,
                filter,
            )
            .await
    }

    async fn medal_times(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<MedalTimes>> {
        let row = self.load_row(ctx).await?;

        let medal_times = event_utils::MedalTimes::from_columns(
            row.bronze_time,
            row.silver_time,
            row.gold_time,
            row.author_time,
        );

        Ok(medal_times.map(From::from))
    }
}
