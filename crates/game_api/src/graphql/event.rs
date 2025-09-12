use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_graphql::dataloader::{DataLoader, Loader};
use deadpool_redis::redis::AsyncCommands;
use entity::{
    event, event_admins, event_categories, event_category, event_edition, event_edition_categories,
    event_edition_maps, global_event_records, global_records, maps, players, records,
};
use futures::{Stream as _, StreamExt as _, TryStreamExt};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, DbErr, EntityTrait, FromQueryResult, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait as _,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func, IntoCondition, Query},
};

use records_lib::{
    RedisPool,
    event::{self as event_utils, EventMap, MedalTimes},
    mappack::AnyMappackId,
    must,
    opt_event::OptEvent,
    redis_key::{mappack_map_last_rank, mappack_player_ranks_key, mappack_time_key},
};

use crate::{RecordsResult, RecordsResultExt, http::event::HasExpireTime as _, internal};

use super::{
    SortState,
    map::{Map, MapLoader},
    mappack::{self, Mappack},
    player::Player,
    record::RankedRecord,
};

#[derive(Debug, Clone, FromQueryResult)]
pub struct Event {
    #[sea_orm(nested)]
    inner: event::Model,
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

    async fn admins(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Player>> {
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

    async fn categories(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<event_category::Model>> {
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
            .all(conn)
            .await?;
        Ok(q)
    }

    async fn editions(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventEdition<'_>>> {
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
    ) -> async_graphql::Result<Option<EventEdition<'_>>> {
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
            .await
            .with_api_err()?;

        Ok(edition.map(|inner| EventEdition {
            event: Cow::Borrowed(self),
            inner,
        }))
    }

    async fn edition(
        &self,
        ctx: &async_graphql::Context<'_>,
        edition_id: u32,
    ) -> async_graphql::Result<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_utils::get_edition_by_id(conn, self.inner.id, edition_id).await?;

        Ok(edition.map(|inner| EventEdition {
            event: Cow::Borrowed(self),
            inner,
        }))
    }
}

pub struct EventLoader(pub DbConn);

impl Loader<u32> for EventLoader {
    type Value = Event;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = event::Entity::find()
            .filter(event::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|row| (row.id, row.into()))
            .collect();
        Ok(hashmap)
    }
}

pub struct EventCategoryLoader(pub DbConn);

impl Loader<u32> for EventCategoryLoader {
    type Value = event_category::Model;
    type Error = Arc<DbErr>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let hashmap = event_category::Entity::find()
            .filter(event_category::Column::Id.is_in(keys.iter().copied()))
            .all(&self.0)
            .await?
            .into_iter()
            .map(|category| (category.id, category))
            .collect();

        Ok(hashmap)
    }
}

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
            .await
            .with_api_err()?
            .ok_or_else(|| internal!("Event {} should be in database", inner.event_id))?;
        Ok(Self {
            event: Cow::Owned(event),
            inner,
        })
    }
}

struct EventEditionPlayer<'a> {
    edition: &'a EventEdition<'a>,
    player: players::Model,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(complex)]
struct EventEditionMap<'a> {
    edition: &'a EventEdition<'a>,
    map: super::map::Map,
}

struct EventEditionPlayerCategorizedRank<'a> {
    player: &'a EventEditionPlayer<'a>,
    category: event_category::Model,
}

struct EventEditionPlayerRank<'a> {
    edition_player: &'a EventEditionPlayer<'a>,
    map_game_id: String,
    record_time: i32,
}

struct EventEditionMapExt<'a> {
    edition_player: &'a EventEditionPlayer<'a>,
    inner: Map,
}

#[async_graphql::Object]
impl EventEditionMapExt<'_> {
    async fn map(&self) -> &Map {
        &self.inner
    }

    async fn last_rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i32> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let last_rank = redis_conn
            .get(mappack_map_last_rank(
                AnyMappackId::Event(
                    &self.edition_player.edition.event.inner,
                    &self.edition_player.edition.inner,
                ),
                &self.inner.inner.game_id,
            ))
            .await?;
        Ok(last_rank)
    }

    async fn medal_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<MedalTimes>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let medal_times = event_utils::get_medal_times_of(
            conn,
            self.edition_player.edition.inner.event_id,
            self.edition_player.edition.inner.id,
            self.inner.inner.id,
        )
        .await?;
        Ok(medal_times)
    }
}

#[async_graphql::Object]
impl EventEditionPlayerRank<'_> {
    async fn rank(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<usize> {
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let redis_conn = &mut redis_pool.get().await?;
        let rank = redis_conn
            .zscore(
                mappack_player_ranks_key(
                    AnyMappackId::Event(
                        &self.edition_player.edition.event.inner,
                        &self.edition_player.edition.inner,
                    ),
                    self.edition_player.player.id,
                ),
                &self.map_game_id,
            )
            .await?;
        Ok(rank)
    }

    async fn time(&self) -> i32 {
        self.record_time
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<EventEditionMapExt<'_>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map = must::have_map(conn, &self.map_game_id).await?;
        Ok(EventEditionMapExt {
            inner: map.into(),
            edition_player: self.edition_player,
        })
    }
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

    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<u64> {
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
    ) -> async_graphql::Result<Vec<EventEditionPlayerRank<'_>>> {
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

#[async_graphql::ComplexObject]
impl EventEditionMap<'_> {
    async fn link_to_original(&self) -> bool {
        !(self.edition.inner.save_non_event_record != 0
            && self.edition.inner.non_original_maps != 0)
            && self.edition.inner.is_transparent == 0
    }

    async fn original_map(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<Map>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let original_map_id = event_edition_maps::Entity::find_by_id((
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
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        self.map
            .get_records(
                ctx,
                OptEvent::new(&self.edition.event.inner, &self.edition.inner),
                rank_sort_by,
                date_sort_by,
            )
            .await
    }

    async fn medal_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<MedalTimes>> {
        let conn = ctx.data_unchecked::<DbConn>();
        let medal_times = event_utils::get_medal_times_of(
            conn,
            self.edition.inner.event_id,
            self.edition.inner.id,
            self.map.inner.id,
        )
        .await?;
        Ok(medal_times)
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
    ) -> async_graphql::Result<Vec<event_category::Model>> {
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
            .all(conn)
            .await?;
        Ok(q)
    }
}
