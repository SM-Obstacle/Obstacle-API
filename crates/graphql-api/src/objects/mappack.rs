use async_graphql::dataloader::DataLoader;
use entity::{mappack_maps, mappack_periodic_ranking, mappack_player_periodic_ranking, mappacks};
use mkenv::prelude::*;
use mx_layer::mappacks::MappackProvider;
use records_lib::{
    Database,
    error::{RecordsError, RecordsResult},
    internal,
    mappack::{AnyMappackId, update_mappack},
    must,
};
use sea_orm::{
    ActiveValue::Set, ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait as _,
    PaginatorTrait as _, QueryFilter as _, QueryOrder as _, QuerySelect, TransactionTrait,
};

use crate::{
    error::GqlResult, loaders::player::PlayerLoader, objects::mappack_player::MappackPlayer,
};

/// Stores a mappack and what ManiaExchange says about it in SQL.
///
/// The two questions are asked side by side, and both go through the
/// [MX layer](mx_layer::mappacks), so a mappack several people land on at the same moment costs
/// ManiaExchange one request instead of one per visitor.
async fn fill_mappack<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    mx: &MappackProvider,
    mappack: AnyMappackId<'_>,
    mappack_id: u32,
) -> RecordsResult<()> {
    let (maps, info) = tokio::join!(mx.tracks(mappack_id, None), mx.info(mappack_id));

    let missing = || internal!("ManiaExchange has no mappack with ID {mappack_id}");
    let maps = maps?.ok_or_else(missing)?;
    let info = info?.ok_or_else(missing)?;

    let mappack_id = mappack.mappack_id().to_string();
    let txn = conn.begin().await?;
    mappacks::Entity::insert(mappacks::ActiveModel {
        id: Set(mappack_id.clone()),
        mx_author: Set(Some(info.creator.name)),
        mx_name: Set(Some(info.name)),
        mx_created_at: Set(Some(info.created)),
        ..Default::default()
    })
    .on_conflict(
        sea_orm::sea_query::OnConflict::column(mappacks::Column::Id)
            .update_columns([
                mappacks::Column::MxAuthor,
                mappacks::Column::MxName,
                mappacks::Column::MxCreatedAt,
            ])
            .to_owned(),
    )
    .exec(&txn)
    .await?;
    mappack_maps::Entity::delete_many()
        .filter(mappack_maps::Column::MappackId.eq(&mappack_id))
        .exec(&txn)
        .await?;
    let mut map_rows = Vec::with_capacity(maps.len());
    for (map_order, mx_map) in maps.into_iter().enumerate() {
        // We check that the map exists in our database
        let map = must::have_map(&txn, &mx_map.map_uid).await?;
        map_rows.push(mappack_maps::ActiveModel {
            mappack_id: Set(mappack_id.clone()),
            map_id: Set(map.id),
            map_order: Set(map_order as u32),
        });
    }
    mappack_maps::Entity::insert_many(map_rows)
        .exec(&txn)
        .await?;
    txn.commit().await?;

    Ok(())
}

pub struct Mappack {
    pub(crate) event_has_expired: bool,
    pub(crate) mappack_id: String,
}

impl From<String> for Mappack {
    fn from(mappack_id: String) -> Self {
        Self {
            mappack_id,
            event_has_expired: false,
        }
    }
}

#[async_graphql::Object]
impl Mappack {
    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<usize> {
        let conn = ctx.data_unchecked::<DbConn>();
        let nb_map = mappack_maps::Entity::find()
            .filter(mappack_maps::Column::MappackId.eq(&self.mappack_id))
            .count(conn)
            .await?;
        Ok(nb_map as usize)
    }

    async fn mx_author(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<String>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let author = mappacks::Entity::find_by_id(&self.mappack_id)
            .one(conn)
            .await?;
        Ok(author.and_then(|m| m.mx_author))
    }

    async fn mx_created_at(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> GqlResult<Option<chrono::NaiveDateTime>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let created_at = mappacks::Entity::find_by_id(&self.mappack_id)
            .one(conn)
            .await?;
        let parsed_date = created_at
            .and_then(|m| m.mx_created_at)
            .map(|s| {
                s.parse().map_err(|e| {
                    internal!(
                        "create timestamp of mappack {} is an invalid timestamp: {e}. got `{s}`",
                        self.mappack_id
                    )
                })
            })
            .transpose()?;

        Ok(parsed_date)
    }

    async fn mx_name(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<String>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let name = mappacks::Entity::find_by_id(&self.mappack_id)
            .one(conn)
            .await?;
        Ok(name.and_then(|m| m.mx_name))
    }

    async fn leaderboard<'a>(
        &'a self,
        ctx: &async_graphql::Context<'_>,
        limit: Option<u64>,
    ) -> GqlResult<Vec<MappackPlayer<'a>>> {
        let db = ctx.data_unchecked::<Database>();
        let player_loader = ctx.data_unchecked::<DataLoader<PlayerLoader>>();
        let Some(snapshot) = mappack_periodic_ranking::Entity::find()
            .filter(mappack_periodic_ranking::Column::MappackId.eq(&self.mappack_id))
            .order_by_desc(mappack_periodic_ranking::Column::PeriodId)
            .one(&db.sql_conn)
            .await?
        else {
            return Ok(Vec::new());
        };
        let scores = mappack_player_periodic_ranking::Entity::find()
            .filter(
                mappack_player_periodic_ranking::Column::PeriodId
                    .eq(snapshot.period_id)
                    .and(mappack_player_periodic_ranking::Column::MappackId.eq(&self.mappack_id)),
            )
            .order_by_asc(mappack_player_periodic_ranking::Column::Rank)
            .order_by_asc(mappack_player_periodic_ranking::Column::PlayerId)
            .limit(limit)
            .all(&db.sql_conn)
            .await?;

        let players = player_loader
            .load_many(scores.iter().map(|score| score.player_id))
            .await?;
        let out = players
            .into_values()
            .map(|player| MappackPlayer {
                inner: player,
                mappack: self,
            })
            .collect();

        Ok(out)
    }

    async fn player<'a>(
        &'a self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> GqlResult<MappackPlayer<'a>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let player = must::have_player(conn, &login).await?;

        Ok(MappackPlayer {
            inner: player.into(),
            mappack: self,
        })
    }

    async fn next_update_in(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Option<u64>> {
        if self.event_has_expired {
            return Ok(None);
        }

        let db = ctx.data_unchecked::<Database>();
        let last_updated_at = mappacks::Entity::find_by_id(&self.mappack_id)
            .one(&db.sql_conn)
            .await?;
        let last_updated_at = last_updated_at.and_then(|m| m.last_updated_at);
        Ok(Some(
            last_updated_at
                .and_then(|last| {
                    let elapsed = chrono::Utc::now().naive_utc() - last;
                    records_lib::env()
                        .event_scores_interval
                        .get()
                        .as_secs()
                        .checked_sub(elapsed.num_seconds().max(0) as u64)
                })
                .unwrap_or_default(),
        ))
    }
}

pub async fn get_mappack(
    ctx: &async_graphql::Context<'_>,
    mappack_id: String,
) -> RecordsResult<Mappack> {
    let db = ctx.data_unchecked::<Database>();

    let mappack = AnyMappackId::Id(&mappack_id);

    let mappack_exists = mappacks::Entity::find_by_id(&mappack_id)
        .one(&db.sql_conn)
        .await?
        .is_some();

    // We load the campaign, and update it, before retrieving the scores from it
    if !mappack_exists {
        let Ok(mappack_id_int) = mappack_id.parse() else {
            return Err(RecordsError::InvalidMappackId(mappack_id));
        };

        let mx = ctx.data_unchecked::<MappackProvider>();

        // We fill the mappack
        fill_mappack(&db.sql_conn, mx, mappack, mappack_id_int).await?;

        // And we update it to have its scores cached
        update_mappack(
            &db.sql_conn,
            &db.redis_pool,
            AnyMappackId::Id(&mappack_id),
            Default::default(),
        )
        .await?;
    }

    Ok(From::from(mappack_id))
}
