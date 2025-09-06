use std::{collections::HashMap, sync::Arc};

use async_graphql::{
    ID,
    dataloader::{DataLoader, Loader},
};
use deadpool_redis::redis::AsyncCommands;
use entity::{
    event_edition, event_edition_maps, global_event_records, global_records, maps, player_rating,
    records,
};
use futures::StreamExt;
use records_lib::{
    Database, RedisConnection,
    opt_event::OptEvent,
    ranks::{get_rank, update_leaderboard},
    redis_key::map_key,
    transaction,
};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DatabaseConnection, DbConn, DbErr, EntityTrait,
    FromQueryResult, QueryFilter, QueryOrder, QuerySelect, StatementBuilder, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait, Func, Query},
};

use crate::internal;

use super::{
    SortState,
    event::EventEdition,
    player::{Player, PlayerLoader},
    rating::PlayerRating,
    record::RankedRecord,
};

#[derive(Debug, Clone, FromQueryResult)]
pub struct Map {
    #[sea_orm(nested)]
    pub inner: maps::Model,
}

impl From<maps::Model> for Map {
    fn from(inner: maps::Model) -> Self {
        Self { inner }
    }
}

async fn get_map_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    map_id: u32,
    event: OptEvent<'_>,
    rank_sort_by: Option<SortState>,
    date_sort_by: Option<SortState>,
) -> async_graphql::Result<Vec<RankedRecord>> {
    let key = map_key(map_id, event);

    update_leaderboard(conn, redis_conn, map_id, event).await?;

    let to_reverse = matches!(rank_sort_by, Some(SortState::Reverse));
    let record_ids: Vec<i32> = if to_reverse {
        redis_conn.zrevrange(&key, 0, 99)
    } else {
        redis_conn.zrange(&key, 0, 99)
    }
    .await?;

    if record_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut select = Query::select();

    let select = match event.event {
        Some((ev, ed)) => select.from_as(global_event_records::Entity, "r").and_where(
            Expr::col(("r", global_event_records::Column::EventId))
                .eq(ev.id)
                .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
        ),
        None => select.from_as(global_records::Entity, "r"),
    }
    .column(Asterisk)
    .and_where(Expr::col(("r", records::Column::MapId)).eq(map_id));

    if let Some(ref s) = date_sort_by {
        select.order_by_expr(
            Expr::col(("r", records::Column::RecordDate)).into(),
            match s {
                SortState::Sort => sea_orm::Order::Desc,
                SortState::Reverse => sea_orm::Order::Asc,
            },
        );
    } else {
        select
            .and_where(Expr::col(("r", records::Column::RecordPlayerId)).is_in(record_ids))
            .order_by_expr(
                Expr::col(("r", records::Column::Time)).into(),
                if to_reverse {
                    sea_orm::Order::Desc
                } else {
                    sea_orm::Order::Asc
                },
            )
            .order_by_expr(
                Expr::col(("r", records::Column::RecordDate)).into(),
                sea_orm::Order::Asc,
            );
    }

    if date_sort_by.is_some() {
        select.limit(100);
    }

    let stmt = StatementBuilder::build(&*select, &conn.get_database_backend());
    let records = conn
        .query_all(stmt)
        .await?
        .into_iter()
        .map(|result| records::Model::from_query_result(&result, ""))
        .collect::<Result<Vec<_>, _>>()?;

    let mut ranked_records = Vec::with_capacity(records.len());

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;
        ranked_records.push(records::RankedRecord { rank, record }.into());
    }

    Ok(ranked_records)
}

impl Map {
    pub(super) async fn get_records(
        &self,
        gql_ctx: &async_graphql::Context<'_>,
        event: OptEvent<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = gql_ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        records_lib::assert_future_send(transaction::within(&db.sql_conn, async |txn| {
            get_map_records(
                txn,
                &mut redis_conn,
                self.inner.id,
                event,
                rank_sort_by,
                date_sort_by,
            )
            .await
        }))
        .await
    }
}

#[derive(async_graphql::SimpleObject)]
struct RelatedEdition<'a> {
    map: Map,
    /// Tells the website to redirect to the event map page instead of the regular map page.
    ///
    /// This avoids to have access to the `/map/X_benchmark` page for example, because a Benchmark
    /// map won't have any record in this context. Thus, it should be redirected to
    /// `/event/benchmark/2/map/X_benchmark`.
    redirect_to_event: bool,
    edition: EventEdition<'a>,
}

#[derive(FromQueryResult)]
struct RawRelatedEdition {
    map_id: u32,
    #[sea_orm(nested)]
    edition: event_edition::Model,
}

#[async_graphql::Object]
impl Map {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Map:{}", self.inner.id))
    }

    async fn game_id(&self) -> &str {
        &self.inner.game_id
    }

    async fn player_id(&self) -> ID {
        ID(format!("v0:Player:{}", self.inner.player_id))
    }

    async fn cps_number(&self) -> Option<u32> {
        self.inner.cps_number
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.inner.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn related_event_editions(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RelatedEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let mut raw_editions = event_edition::Entity::find()
            .reverse_join(event_edition_maps::Entity)
            .filter(Expr::val(self.inner.id).is_in([
                Expr::col(event_edition_maps::Column::MapId),
                Expr::col(event_edition_maps::Column::OriginalMapId),
            ]))
            .order_by_desc(event_edition::Column::StartDate)
            .column(event_edition_maps::Column::MapId)
            .into_model::<RawRelatedEdition>()
            .stream(conn)
            .await?;

        let mut out = Vec::with_capacity(raw_editions.size_hint().0);

        while let Some(raw_edition) = raw_editions.next().await {
            let edition = raw_edition?;
            out.push(RelatedEdition {
                map: map_loader
                    .load_one(edition.map_id)
                    .await?
                    .ok_or_else(|| internal!("unknown map id: {}", edition.map_id))?,
                // We want to redirect to the event map page if the edition saves any records
                // on its maps, doesn't have any original map like campaign, or if the map
                // isn't the original one.
                // TODO: this shouldn't be decided by the API actually
                redirect_to_event: edition.edition.is_transparent == 0
                    && edition.edition.save_non_event_record != 0
                    && (edition.edition.non_original_maps != 0 || self.inner.id == edition.map_id),
                edition: EventEdition::from_inner(conn, edition.edition).await?,
            });
        }

        Ok(out)
    }

    async fn average_rating(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<PlayerRating>> {
        let conn = ctx.data_unchecked::<DatabaseConnection>();
        let all = player_rating::Entity::find()
            .filter(player_rating::Column::MapId.eq(self.inner.id))
            .select_only()
            .expr_as(1.cast_as("UNSIGNED"), "player_id")
            .columns([player_rating::Column::MapId, player_rating::Column::Kind])
            .expr_as(
                Func::avg(Expr::col(player_rating::Column::Rating)),
                "rating",
            )
            .into_model()
            .all(conn)
            .await?;
        Ok(all)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        self.get_records(ctx, Default::default(), rank_sort_by, date_sort_by)
            .await
    }
}

pub struct MapLoader(pub DbConn);

impl Loader<u32> for MapLoader {
    type Value = Map;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = maps::Entity::find()
            .filter(maps::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|map| (map.id, map.into()))
            .collect();

        Ok(hashmap)
    }
}
