use std::{collections::HashMap, sync::Arc};

use async_graphql::{
    dataloader::{DataLoader, Loader},
    Context, ID,
};
use deadpool_redis::{redis::AsyncCommands, Pool as RedisPool};
use sqlx::{mysql, FromRow, MySqlPool, Row};

use crate::{
    models::{Map, MedalPrice, Player, PlayerRating, RankedRecord, Rating, Record},
    redis, Database,
};

use super::player::PlayerLoader;

#[async_graphql::Object]
impl Map {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Map:{}", self.id))
    }

    async fn game_id(&self) -> String {
        self.game_id.clone()
    }

    async fn player_id(&self) -> ID {
        ID(format!("v0:Player:{}", self.player_id))
    }

    async fn cps_number(&self, ctx: &Context<'_>) -> async_graphql::Result<i64> {
        Ok(ctx
            .data_unchecked::<DataLoader<CpsNumberLoader>>()
            .load_one(self.id)
            .await?
            .unwrap_or(0))
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<DataLoader<PlayerLoader>>()
            .load_one(self.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn medal_for(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<Option<MedalPrice>> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let player_id = sqlx::query_scalar!("SELECT id FROM players WHERE login = ?", login)
            .fetch_one(db)
            .await?;

        Ok(sqlx::query_as(
            "SELECT * FROM prizes WHERE map_id = ? AND player_id = ? ORDER BY medal DESC",
        )
        .bind(self.id)
        .bind(player_id)
        .fetch_optional(db)
        .await?)
    }

    async fn ratings(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Rating>> {
        let db = ctx.data_unchecked();
        Ok(sqlx::query_as("SELECT * FROM rating WHERE map_id = ?")
            .bind(self.id)
            .fetch_all(db)
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
        .bind(self.id)
        .fetch_all(db)
        .await?;
        Ok(fetch_all)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let mut redis_conn = redis_pool.get().await.unwrap();

        let key = format!("l0:{}", self.game_id);

        redis::update_leaderboard(db, &key, self.id).await?;

        let record_ids: Vec<i32> = redis_conn.zrange(key, 0, 99).await.unwrap();
        if record_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Query the records with these ids
        let query = format!(
            "SELECT * FROM records WHERE map_id = ? AND player_id IN ({}) ORDER BY time ASC",
            record_ids
                .iter()
                .map(|_| "?".to_string())
                .collect::<Vec<String>>()
                .join(",")
        );

        let mut query = sqlx::query(&query);
        query = query.bind(self.id);
        for id in &record_ids {
            query = query.bind(id);
        }
        let mut records = query
            .map(|row: mysql::MySqlRow| RankedRecord {
                rank: 0,
                record: Record::from_row(&row).unwrap(),
            })
            .fetch_all(mysql_pool)
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();

        let mut rank = 0;
        for mut record in &mut records {
            record.rank = rank + 1;
            rank += 1;
        }

        Ok(records)
    }
}

pub struct MapLoader(pub MySqlPool);

#[async_graphql::async_trait::async_trait]
impl Loader<u32> for MapLoader {
    type Value = Map;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let query = format!(
            "SELECT * FROM maps WHERE id IN ({})",
            keys.iter()
                .map(|_| "?".to_string())
                .collect::<Vec<String>>()
                .join(",")
        );

        let mut query = sqlx::query(&query);

        for key in keys {
            query = query.bind(key);
        }

        Ok(query
            .map(|row: mysql::MySqlRow| {
                let map = Map::from_row(&row).unwrap();
                (map.id, map)
            })
            .fetch_all(&self.0)
            .await?
            .into_iter()
            .collect())
    }
}

pub struct CpsNumberLoader(pub MySqlPool);

#[async_graphql::async_trait::async_trait]
impl Loader<u32> for CpsNumberLoader {
    type Value = i64;

    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let query = format!(
            "SELECT COUNT(*) FROM checkpoint WHERE map_id IN ({})",
            keys.iter()
                .map(|_| "?".to_owned())
                .collect::<Vec<String>>()
                .join(",")
        );

        let mut query = sqlx::query(&query);

        for key in keys {
            query = query.bind(key);
        }

        Ok(keys
            .iter()
            .copied()
            .zip(
                query
                    .map(|row| row.get(0))
                    .fetch_all(&self.0)
                    .await?
                    .into_iter(),
            )
            .collect())
    }
}
