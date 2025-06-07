use std::{borrow::Cow, collections::HashMap, iter::repeat_n, sync::Arc};

use async_graphql::dataloader::{DataLoader, Loader};
use deadpool_redis::redis::AsyncCommands;
use futures::{StreamExt as _, TryStreamExt};
use sqlx::{FromRow, MySqlPool, Row, mysql};

use records_lib::{
    RedisPool,
    event::{self, EventMap, MedalTimes},
    mappack::AnyMappackId,
    models::{self, EventCategory},
    must,
    opt_event::OptEvent,
    redis_key::{mappack_map_last_rank, mappack_player_ranks_key},
};

use crate::{RecordsResult, RecordsResultExt};

use super::{
    SortState,
    map::{Map, MapLoader},
    mappack::{self, Mappack},
    player::Player,
    record::RankedRecord,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Event {
    #[sqlx(flatten)]
    inner: models::Event,
}

impl From<models::Event> for Event {
    fn from(inner: models::Event) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Event {
    async fn handle(&self) -> &str {
        &self.inner.handle
    }

    async fn cooldown(&self) -> Option<u8> {
        self.inner.cooldown
    }

    async fn admins(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Player>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let q = sqlx::query_as(
            "SELECT * FROM players
            WHERE id IN (
                SELECT player_id FROM event_admins
                WHERE event_id = ?
            )",
        )
        .bind(self.inner.id)
        .fetch_all(db)
        .await?;

        Ok(q)
    }

    async fn categories(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventCategory>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let q = sqlx::query_as(
            "SELECT * FROM event_category
            WHERE id IN (
                SELECT category_id FROM event_categories
                WHERE event_id = ?
            )",
        )
        .bind(self.inner.id)
        .fetch_all(db)
        .await?;
        Ok(q)
    }

    async fn editions(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventEdition>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;

        let q = event::event_editions_list(&mut mysql_conn, &self.inner.handle).await?;

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
    ) -> async_graphql::Result<Option<EventEdition>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let edition: Option<models::EventEdition> = sqlx::query_as(
            "select ee.* from event_edition ee
            inner join event_edition_maps eem on ee.id = eem.edition_id
                and ee.event_id = eem.event_id
            where ee.event_id = ? and ee.start_date < sysdate() and
                (ee.ttl is null or ee.start_date + interval ee.ttl second > sysdate())
            order by ee.id desc
            limit 1",
        )
        .bind(self.inner.id)
        .fetch_optional(db)
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
    ) -> async_graphql::Result<Option<EventEdition>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;
        let edition = event::get_edition_by_id(&mut mysql_conn, self.inner.id, edition_id).await?;

        Ok(edition.map(|inner| EventEdition {
            event: Cow::Borrowed(self),
            inner,
        }))
    }
}

pub struct EventLoader(pub MySqlPool);

impl Loader<u32> for EventLoader {
    type Value = Event;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let q = format!(
            "SELECT * FROM event WHERE id IN ({})",
            repeat_n("?".to_string(), keys.len())
                .collect::<Vec<_>>()
                .join(",")
        );

        let mut q = sqlx::query(&q);

        for key in keys {
            q = q.bind(key);
        }

        Ok(q.map(|row: mysql::MySqlRow| {
            let event = models::Event::from_row(&row).unwrap();
            (event.id, event.into())
        })
        .fetch_all(&self.0)
        .await?
        .into_iter()
        .collect())
    }
}

pub struct EventCategoryLoader(pub MySqlPool);

impl Loader<u32> for EventCategoryLoader {
    type Value = EventCategory;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let q = format!(
            "SELECT * FROM event_category WHERE id IN ({})",
            repeat_n("?".to_string(), keys.len())
                .collect::<Vec<_>>()
                .join(",")
        );

        let mut q = sqlx::query(&q);

        for key in keys {
            q = q.bind(key);
        }

        Ok(q.map(|row: mysql::MySqlRow| {
            let category = EventCategory::from_row(&row).unwrap();
            (category.id, category)
        })
        .fetch_all(&self.0)
        .await?
        .into_iter()
        .collect())
    }
}

#[derive(Debug, Clone)]
pub struct EventEdition<'a> {
    pub(super) event: Cow<'a, Event>,
    pub(super) inner: models::EventEdition,
}

impl EventEdition<'_> {
    pub(super) async fn from_inner(
        inner: models::EventEdition,
        db: &MySqlPool,
    ) -> RecordsResult<Self> {
        let event = sqlx::query_as("select * from event where id = ?")
            .bind(inner.event_id)
            .fetch_one(db)
            .await
            .with_api_err()?;
        Ok(Self {
            event: Cow::Owned(event),
            inner,
        })
    }
}

struct EventEditionPlayer<'a> {
    edition: &'a EventEdition<'a>,
    player: models::Player,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(complex)]
struct EventEditionMap<'a> {
    edition: &'a EventEdition<'a>,
    map: super::map::Map,
}

struct EventEditionPlayerCategorizedRank<'a> {
    player: &'a EventEditionPlayer<'a>,
    category: models::EventCategory,
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
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut db = db.acquire().await?;
        let medal_times = event::get_medal_times_of(
            &mut db,
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
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;
        let map = must::have_map(&mut mysql_conn, &self.map_game_id).await?;
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

    async fn nb_maps(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i64> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let edition = &self.player.edition.inner;

        let n = sqlx::query_scalar(
            "select count(*) from event_edition_maps eem
            inner join event_edition ee on eem.edition_id = ee.id and eem.event_id = ee.event_id
            inner join event_category ec on eem.category_id = ec.id
            where eem.event_id = ? and eem.edition_id = ? and ec.id = ?",
        )
        .bind(edition.event_id)
        .bind(edition.id)
        .bind(self.category.id)
        .fetch_one(db)
        .await?;

        Ok(n)
    }

    async fn ranks(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<EventEditionPlayerRank>> {
        let db = ctx.data_unchecked::<MySqlPool>();

        if self.category.id == 0 {
            let res = sqlx::query(
                "select m.game_id, r.time from global_event_records r
                inner join maps m on m.id = r.map_id
                inner join event_edition_maps eem on eem.map_id = m.id
                where r.event_id = ? and r.edition_id = ? and r.record_player_id = ?
                order by eem.`order`",
            )
            .bind(self.player.edition.inner.event_id)
            .bind(self.player.edition.inner.id)
            .bind(self.player.player.id)
            .map(|row: sqlx::mysql::MySqlRow| {
                let game_id = row.get("game_id");
                let time = row.get("time");
                EventEditionPlayerRank {
                    map_game_id: game_id,
                    record_time: time,
                    edition_player: self.player,
                }
            })
            .fetch_all(db)
            .await?;

            return Ok(res);
        }

        let res = sqlx::query(
            "select m.game_id as game_id, r.time as time
            from global_event_records r
            inner join event_edition_maps eem on eem.map_id = r.map_id
            inner join maps m on m.id = eem.map_id
            inner join event_category ec on eem.category_id = ec.id
            where eem.event_id = ? and eem.edition_id = ? and r.record_player_id = ? and ec.id = ?
            order by eem.`order`",
        )
        .bind(self.player.edition.inner.event_id)
        .bind(self.player.edition.inner.id)
        .bind(self.player.player.id)
        .bind(self.category.id)
        .map(|row: mysql::MySqlRow| {
            let game_id = row.get("game_id");
            let time = row.get("time");
            EventEditionPlayerRank {
                map_game_id: game_id,
                record_time: time,
                edition_player: self.player,
            }
        })
        .fetch_all(db)
        .await?;

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
    ) -> async_graphql::Result<Vec<EventEditionPlayerCategorizedRank>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;
        let categories = event::get_categories_by_edition_id(
            &mut mysql_conn,
            self.edition.inner.event_id,
            self.edition.inner.id,
        )
        .await?;

        let categories = if categories.is_empty() {
            vec![models::EventCategory {
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
        let db = ctx.data_unchecked::<MySqlPool>();

        let mut unfinished_maps = sqlx::query_as(
            "select m.* from maps m
            inner join event_edition_maps eem on m.id = eem.map_id
            left join records gr on m.id = gr.map_id and gr.record_player_id = ?
            left join event_edition_records eer on gr.record_id = eer.record_id
            where eem.event_id = ? and eem.edition_id = ? and gr.record_id is null",
        )
        .bind(self.player.id)
        .bind(self.edition.inner.event_id)
        .bind(self.edition.inner.id)
        .fetch(db);

        let mut out = Vec::with_capacity(unfinished_maps.size_hint().0);

        while let Some(map) = unfinished_maps.next().await {
            out.push(EventEditionMapExt {
                edition_player: self,
                inner: From::<models::Map>::from(map?),
            });
        }

        Ok(out)
    }
}

#[async_graphql::ComplexObject]
impl EventEditionMap<'_> {
    async fn link_to_original(&self) -> bool {
        !(self.edition.inner.save_non_event_record && self.edition.inner.non_original_maps)
    }

    async fn original_map(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<Map>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();
        let mut conn = db.acquire().await?;

        let original_map_id: Option<_> = sqlx::query_scalar(
            "select original_map_id from event_edition_maps
            where event_id = ? and edition_id = ? and map_id = ?",
        )
        .bind(self.edition.inner.event_id)
        .bind(self.edition.inner.id)
        .bind(self.map.inner.id)
        .fetch_one(&mut *conn)
        .await?;

        let map = match original_map_id {
            Some(id) => Some(
                map_loader
                    .load_one(id)
                    .await?
                    .expect("unknown original_map_id"),
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
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut db = db.acquire().await?;
        let medal_times = event::get_medal_times_of(
            &mut db,
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

    async fn mappack(&self) -> Option<Mappack> {
        Some(Mappack {
            mappack_id: AnyMappackId::Event(&self.event.inner, &self.inner)
                .mappack_id()
                .to_string(),
            event_has_expired: self.inner.has_expired(),
        })
    }

    async fn admins(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Player>> {
        let mut db = ctx.data_unchecked::<MySqlPool>().acquire().await?;
        let a = event::get_admins_of(&mut db, self.inner.event_id, self.inner.id)
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
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;
        let player = must::have_player(&mut mysql_conn, &login).await?;
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
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut mysql_conn = db.acquire().await?;

        let EventMap { map, .. } = event::get_map_in_edition(
            &mut mysql_conn,
            &game_id,
            self.inner.event_id,
            self.inner.id,
        )
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
    ) -> async_graphql::Result<Vec<EventCategory>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let q = sqlx::query_as(
            "SELECT * FROM event_category
            WHERE id IN (
                SELECT category_id FROM event_edition_categories
                WHERE event_id = ? AND edition_id = ?
            )",
        )
        .bind(self.inner.event_id)
        .bind(self.inner.id)
        .fetch_all(db)
        .await?;
        Ok(q)
    }
}
