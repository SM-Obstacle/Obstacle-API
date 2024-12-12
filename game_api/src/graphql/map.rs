use std::{collections::HashMap, iter::repeat, sync::Arc};

use async_graphql::{
    dataloader::{DataLoader, Loader},
    ID,
};
use deadpool_redis::redis::AsyncCommands;
use futures::StreamExt;
use records_lib::{
    acquire,
    context::{Context, Ctx},
    models::{self, Record},
    ranks::{get_rank, update_leaderboard},
    redis_key::alone_map_key,
    Database,
};
use sqlx::{mysql, FromRow, MySqlPool};

use crate::auth::{self, privilege, WebToken};

use super::{
    event::EventEdition,
    player::{Player, PlayerLoader},
    rating::{PlayerRating, Rating},
    record::RankedRecord,
    SortState,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Map {
    #[sqlx(flatten)]
    pub inner: models::Map,
}

impl From<models::Map> for Map {
    fn from(inner: models::Map) -> Self {
        Self { inner }
    }
}

impl Map {
    pub(super) async fn get_records<C: Ctx>(
        &self,
        gql_ctx: &async_graphql::Context<'_>,
        ctx: C,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let ctx = ctx.with_map(&self.inner);
        let db = gql_ctx.data_unchecked::<Database>();
        let mut conn = acquire!(db?);

        let key = alone_map_key(self.inner.id);

        update_leaderboard(&mut conn, &ctx).await?;

        let to_reverse = matches!(rank_sort_by, Some(SortState::Reverse));
        let record_ids: Vec<i32> = if to_reverse {
            conn.redis_conn.zrevrange(&key, 0, 99)
        } else {
            conn.redis_conn.zrange(&key, 0, 99)
        }
        .await?;
        if record_ids.is_empty() {
            return Ok(Vec::new());
        }

        let builder = ctx.sql_frag_builder();

        let mut query = sqlx::QueryBuilder::new("SELECT * FROM ");
        builder
            .push_event_view_name(&mut query, "r")
            .push(" where r.map_id = ")
            .push_bind(self.inner.id)
            .push(" ");

        if let Some(ref s) = date_sort_by {
            query.push("order by");
            if let SortState::Reverse = s {
                query.push(" asc");
            } else {
                query.push(" desc");
            }
            query.push(", r.record_date asc");
        } else {
            let mut sep = builder
                .push_event_filter(&mut query, "r")
                .push(" and r.record_player_id in (")
                .separated(", ");
            for id in record_ids {
                sep.push_bind(id);
            }
            query.push(") order by r.time");
            if to_reverse {
                query.push(" desc");
            } else {
                query.push(" asc");
            }
            query.push(", r.record_date asc");
        }

        // "SELECT * FROM global_edition_records r where r.map_id = ? and r.record_player_id in (r.event_id = ? and r.edition_id = ??, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) order by r.time asc, r.record_date asc"

        if date_sort_by.is_some() {
            query.push(" limit 100");
        }

        let mut records = query.build_query_as::<Record>().fetch(&db.mysql_pool);

        let mut ranked_records = Vec::with_capacity(records.size_hint().0);

        while let Some(record) = records.next().await {
            let record = record?;
            let rank = get_rank(
                &mut conn,
                ctx.by_ref().with_player_id(record.record_player_id),
                record.time,
            )
            .await?;

            ranked_records.push(models::RankedRecord { rank, record }.into());
        }

        Ok(ranked_records)
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

#[derive(sqlx::FromRow)]
struct RawRelatedEdition {
    map_id: u32,
    #[sqlx(flatten)]
    edition: models::EventEdition,
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
    ) -> async_graphql::Result<Vec<RelatedEdition>> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let map_loader = ctx.data_unchecked::<DataLoader<MapLoader>>();

        let mut mysql_conn = mysql_pool.acquire().await?;

        let mut raw_editions = sqlx::query_as::<_, RawRelatedEdition>(
            "select ee.*, eem.map_id from event_edition ee
            inner join event_edition_maps eem on ee.id = eem.edition_id and ee.event_id = eem.event_id
            where ? in (eem.map_id, eem.original_map_id)
            order by ee.start_date desc")
        .bind(self.inner.id)
        .fetch(&mut *mysql_conn);

        let mut out = Vec::with_capacity(raw_editions.size_hint().0);

        while let Some(raw_edition) = raw_editions.next().await {
            let edition = raw_edition?;
            out.push(RelatedEdition {
                map: map_loader
                    .load_one(edition.map_id)
                    .await?
                    .expect("unknown map id"),
                // We want to redirect to the event map page if the edition saves any records
                // on its maps, doesn't have any original map like campaign, or if the map
                // isn't the original one.
                redirect_to_event: edition.edition.save_non_event_record
                    && (edition.edition.non_original_maps || self.inner.id == edition.map_id),
                edition: EventEdition::from_inner(edition.edition, mysql_pool).await?,
            });
        }

        Ok(out)
    }

    async fn ratings(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Rating>> {
        let db: &Database = ctx.data_unchecked();
        let Some(WebToken { login, token }) = ctx.data_opt::<WebToken>() else {
            return Err(async_graphql::Error::new("Unauthorized."));
        };

        let author_login: String = sqlx::query_scalar("SELECT login FROM player WHERE id = ?")
            .bind(self.inner.player_id)
            .fetch_one(&db.mysql_pool)
            .await?;

        let role = if author_login != *login {
            privilege::ADMIN
        } else {
            privilege::PLAYER
        };

        auth::website_check_auth_for(db, login, token, role).await?;

        Ok(sqlx::query_as("SELECT * FROM rating WHERE map_id = ?")
            .bind(self.inner.id)
            .fetch_all(&db.mysql_pool)
            .await?)
    }

    async fn average_rating(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<PlayerRating>> {
        let db = ctx.data_unchecked();
        let fetch_all = sqlx::query_as(
            r#"SELECT CAST(0 AS UNSIGNED) AS "player_id", map_id, kind,
            AVG(rating) AS "rating" FROM player_rating
            WHERE map_id = ? GROUP BY kind ORDER BY kind"#,
        )
        .bind(self.inner.id)
        .fetch_all(db)
        .await?;
        Ok(fetch_all)
    }

    #[inline(always)]
    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        rank_sort_by: Option<SortState>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        self.get_records(ctx, Context::default(), rank_sort_by, date_sort_by)
            .await
    }
}

pub struct MapLoader(pub MySqlPool);

impl Loader<u32> for MapLoader {
    type Value = Map;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let query = format!(
            "SELECT * FROM maps WHERE id IN ({})",
            repeat("?".to_string())
                .take(keys.len())
                .collect::<Vec<_>>()
                .join(",")
        );

        let mut query = sqlx::query(&query);

        for key in keys {
            query = query.bind(key);
        }

        Ok(query
            .map(|row: mysql::MySqlRow| {
                let map = Map::from_row(&row).unwrap();
                (map.inner.id, map)
            })
            .fetch_all(&self.0)
            .await?
            .into_iter()
            .collect())
    }
}
