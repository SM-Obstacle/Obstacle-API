use entity::{
    mappack_map_periodic_ranking, mappack_periodic_ranking, mappack_player_periodic_ranking, maps,
};
use records_lib::{Database, internal, mappack::AnyMappackId};
use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _, QueryOrder as _};

use crate::{
    error::GqlResult,
    objects::{mappack::Mappack, mappack_map::MappackMap, player::Player},
};

pub struct MappackPlayer<'a> {
    pub mappack: &'a Mappack,
    pub inner: Player,
}

pub(super) async fn player_rank(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> GqlResult<usize> {
    let db = ctx.data_unchecked::<Database>();
    let Some(snapshot) = latest_snapshot(&db.sql_conn, mappack).await? else {
        return Ok(0);
    };
    Ok(mappack_player_periodic_ranking::Entity::find()
        .filter(
            mappack_player_periodic_ranking::Column::PeriodId
                .eq(snapshot.period_id)
                .and(
                    mappack_player_periodic_ranking::Column::MappackId
                        .eq(mappack.mappack_id().to_string()),
                )
                .and(mappack_player_periodic_ranking::Column::PlayerId.eq(player_id)),
        )
        .one(&db.sql_conn)
        .await?
        .map_or(0, |score| score.rank as usize))
}

pub(super) async fn player_rank_avg(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> GqlResult<f64> {
    let db = ctx.data_unchecked::<Database>();
    let Some(snapshot) = latest_snapshot(&db.sql_conn, mappack).await? else {
        return Ok(0.);
    };
    Ok(mappack_player_periodic_ranking::Entity::find()
        .filter(
            mappack_player_periodic_ranking::Column::PeriodId
                .eq(snapshot.period_id)
                .and(
                    mappack_player_periodic_ranking::Column::MappackId
                        .eq(mappack.mappack_id().to_string()),
                )
                .and(mappack_player_periodic_ranking::Column::PlayerId.eq(player_id)),
        )
        .one(&db.sql_conn)
        .await?
        .map_or(0., |score| score.rank_average))
}

pub(super) async fn player_map_finished(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> GqlResult<usize> {
    let db = ctx.data_unchecked::<Database>();
    let Some(snapshot) = latest_snapshot(&db.sql_conn, mappack).await? else {
        return Ok(0);
    };
    Ok(mappack_player_periodic_ranking::Entity::find()
        .filter(
            mappack_player_periodic_ranking::Column::PeriodId
                .eq(snapshot.period_id)
                .and(
                    mappack_player_periodic_ranking::Column::MappackId
                        .eq(mappack.mappack_id().to_string()),
                )
                .and(mappack_player_periodic_ranking::Column::PlayerId.eq(player_id)),
        )
        .one(&db.sql_conn)
        .await?
        .map_or(0, |score| score.maps_finished as usize))
}

pub(super) async fn player_worst_rank(
    ctx: &async_graphql::Context<'_>,
    mappack: AnyMappackId<'_>,
    player_id: u32,
) -> GqlResult<i32> {
    let db = ctx.data_unchecked::<Database>();
    let Some(snapshot) = latest_snapshot(&db.sql_conn, mappack).await? else {
        return Ok(0);
    };
    Ok(mappack_player_periodic_ranking::Entity::find()
        .filter(
            mappack_player_periodic_ranking::Column::PeriodId
                .eq(snapshot.period_id)
                .and(
                    mappack_player_periodic_ranking::Column::MappackId
                        .eq(mappack.mappack_id().to_string()),
                )
                .and(mappack_player_periodic_ranking::Column::PlayerId.eq(player_id)),
        )
        .one(&db.sql_conn)
        .await?
        .map_or(0, |score| score.worst_rank as i32))
}

pub(super) async fn latest_snapshot<C: sea_orm::ConnectionTrait>(
    conn: &C,
    mappack: AnyMappackId<'_>,
) -> GqlResult<Option<mappack_periodic_ranking::Model>> {
    Ok(mappack_periodic_ranking::Entity::find()
        .filter(mappack_periodic_ranking::Column::MappackId.eq(mappack.mappack_id().to_string()))
        .order_by_desc(mappack_periodic_ranking::Column::PeriodId)
        .one(conn)
        .await?)
}

#[async_graphql::Object]
impl MappackPlayer<'_> {
    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        player_rank(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn player(&self) -> &Player {
        &self.inner
    }

    async fn ranks(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<MappackMap>> {
        let db = ctx.data_unchecked::<Database>();
        let Some(snapshot) =
            latest_snapshot(&db.sql_conn, AnyMappackId::Id(&self.mappack.mappack_id)).await?
        else {
            return Ok(Vec::new());
        };
        let scores = mappack_map_periodic_ranking::Entity::find()
            .filter(
                mappack_map_periodic_ranking::Column::PeriodId
                    .eq(snapshot.period_id)
                    .and(
                        mappack_map_periodic_ranking::Column::MappackId
                            .eq(&self.mappack.mappack_id),
                    )
                    .and(mappack_map_periodic_ranking::Column::PlayerId.eq(self.inner.inner.id)),
            )
            .order_by_asc(mappack_map_periodic_ranking::Column::Rank)
            .all(&db.sql_conn)
            .await?;

        let mut out = Vec::with_capacity(scores.len());

        for score in scores {
            let map = maps::Entity::find_by_id(score.map_id)
                .one(&db.sql_conn)
                .await?
                .ok_or_else(|| internal!("map {} should be in database", score.map_id))?;

            out.push(MappackMap {
                map: map.into(),
                rank: score.rank as i32,
                last_rank: score.last_rank as i32,
            });
        }

        Ok(out)
    }

    async fn rank_avg(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<f64> {
        player_rank_avg(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn map_finished(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        player_map_finished(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }

    async fn worst_rank(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<i32> {
        player_worst_rank(
            ctx,
            AnyMappackId::Id(&self.mappack.mappack_id),
            self.inner.inner.id,
        )
        .await
    }
}
