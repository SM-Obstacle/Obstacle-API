use entity::{mappack_map_periodic_ranking, maps};
use records_lib::{mappack::AnyMappackId, must};
use sea_orm::{ColumnTrait as _, DbConn, EntityTrait as _, QueryFilter as _};

use crate::{
    error::GqlResult,
    objects::{
        event_edition_map_ext::EventEditionMapExt, event_edition_player::EventEditionPlayer,
    },
};

pub struct EventEditionPlayerRank<'a> {
    pub edition_player: &'a EventEditionPlayer<'a>,
    pub map_game_id: String,
    pub record_time: i32,
}

#[async_graphql::Object]
impl EventEditionPlayerRank<'_> {
    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        let db = ctx.data_unchecked::<records_lib::Database>();
        let mappack = AnyMappackId::Event(
            &self.edition_player.edition.event.inner,
            &self.edition_player.edition.inner,
        );
        let Some(snapshot) = super::mappack_player::latest_snapshot(&db.sql_conn, mappack).await?
        else {
            return Ok(0);
        };
        let Some(map) = maps::Entity::find()
            .filter(maps::Column::GameId.eq(&self.map_game_id))
            .one(&db.sql_conn)
            .await?
        else {
            return Ok(0);
        };
        Ok(mappack_map_periodic_ranking::Entity::find()
            .filter(
                mappack_map_periodic_ranking::Column::PeriodId
                    .eq(snapshot.period_id)
                    .and(mappack_map_periodic_ranking::Column::MappackId.eq(snapshot.mappack_id))
                    .and(mappack_map_periodic_ranking::Column::MapId.eq(map.id))
                    .and(
                        mappack_map_periodic_ranking::Column::PlayerId
                            .eq(self.edition_player.player.id),
                    ),
            )
            .one(&db.sql_conn)
            .await?
            .map_or(0, |score| score.rank as usize))
    }

    async fn time(&self) -> i32 {
        self.record_time
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<EventEditionMapExt<'_>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map = must::have_map(conn, &self.map_game_id).await?;
        Ok(EventEditionMapExt {
            inner: map.into(),
            edition_player: self.edition_player,
        })
    }
}
