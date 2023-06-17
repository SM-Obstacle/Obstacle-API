use std::{collections::HashMap, iter::repeat, sync::Arc};

use async_graphql::{
    dataloader::{DataLoader, Loader},
    ID,
};
use deadpool_redis::{redis::AsyncCommands, Pool as RedisPool};
use sqlx::{mysql, FromRow, MySqlPool};

use crate::{
    auth::{self, AuthHeader},
    models::{Map, MedalPrice, Player, PlayerRating, RankedRecord, Rating, Record, Role},
    redis,
    utils::format_map_key,
};

use super::{player::PlayerLoader, utils::get_rank_of};

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

    async fn cps_number(&self) -> Option<u32> {
        self.cps_number
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
        let Some(auth_header) = ctx.data_unchecked::<Option<AuthHeader>>() else {
            return Err(async_graphql::Error::new("Forbidden."));
        };

        let role = if auth_header.login != login {
            Role::Admin
        } else {
            Role::Player
        };

        let db = ctx.data_unchecked();

        auth::check_auth_for(db, auth_header.clone(), role).await?;

        let player_id: u32 = sqlx::query_scalar("SELECT id FROM players WHERE login = ?")
            .bind(login)
            .fetch_one(&db.mysql_pool)
            .await?;

        Ok(sqlx::query_as(
            "SELECT * FROM prizes WHERE map_id = ? AND player_id = ? ORDER BY medal DESC",
        )
        .bind(self.id)
        .bind(player_id)
        .fetch_optional(&db.mysql_pool)
        .await?)
    }

    async fn ratings(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Rating>> {
        let db = ctx.data_unchecked();
        let Some(auth_header) = ctx.data_unchecked::<Option<AuthHeader>>() else {
            return Err(async_graphql::Error::new("Forbidden."));
        };

        let role = if self.player(ctx).await?.login != auth_header.login {
            Role::Admin
        } else {
            Role::Player
        };

        auth::check_auth_for(db, auth_header.clone(), role).await?;

        Ok(sqlx::query_as("SELECT * FROM rating WHERE map_id = ?")
            .bind(self.id)
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
        .bind(self.id)
        .fetch_all(db)
        .await?;
        Ok(fetch_all)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked();
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let mut redis_conn = redis_pool.get().await?;

        let key = format_map_key(self.id);

        redis::update_leaderboard(db, &key, self.id).await?;

        let record_ids: Vec<i32> = redis_conn.zrange(&key, 0, 99).await?;
        if record_ids.is_empty() {
            return Ok(Vec::new());
        }

        let player_ids_query = record_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

        // Query the records with these ids
        let query = format!(
            "SELECT r.*
            FROM records r
            INNER JOIN (
                SELECT MAX(id) AS id, player_id
                FROM records
                WHERE map_id = ? AND player_id IN ({})
                GROUP BY player_id
            ) t ON t.id = r.id AND t.player_id = r.player_id
            WHERE map_id = ? AND r.player_id IN ({})
            ORDER BY r.time ASC, r.record_date ASC",
            player_ids_query, player_ids_query
        );

        println!("{:?}", query);

        let mut query = sqlx::query_as::<_, Record>(&query).bind(self.id);
        for id in &record_ids {
            query = query.bind(id);
        }
        query = query.bind(self.id);
        for id in &record_ids {
            query = query.bind(id);
        }
        let records = query.fetch_all(mysql_pool).await?;
        let mut ranked_records = Vec::with_capacity(records.len());

        for record in records {
            let rank = get_rank_of(&mut redis_conn, &key, record.time)
                .await?
                .unwrap_or_else(|| {
                    panic!("redis leaderboard for (`{key}`) should be updated at this point")
                });

            ranked_records.push(RankedRecord { rank, record });
        }

        Ok(ranked_records)
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
                (map.id, map)
            })
            .fetch_all(&self.0)
            .await?
            .into_iter()
            .collect())
    }
}
