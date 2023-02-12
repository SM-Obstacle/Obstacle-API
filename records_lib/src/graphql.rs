use async_graphql::{connection, dataloader, ID};
use deadpool_redis::redis::AsyncCommands;
use sqlx::{mysql, FromRow, MySqlPool, Row};
use std::{collections::HashMap, sync::Arc, vec::Vec};

use crate::models::*;

/// --- Utils
pub fn decode_id(id: Option<&ID>) -> Option<u32> {
    let parts: Vec<&str> = id?.split(':').collect();
    if parts.len() != 3 || parts[0] != "v0" || (parts[1] != "Map" && parts[1] != "Player") {
        println!(
            "invalid, len: {}, [0]: {}, [1]: {}",
            parts.len(),
            parts[0],
            parts[1]
        );
        None
    } else {
        parts[2].parse::<u32>().ok()
    }
}

pub fn connections_append_query_string_page(
    query: &mut String, has_where_clause: bool, after: Option<u32>, before: Option<u32>,
) {
    if before.is_some() || after.is_some() {
        query.push_str(if !has_where_clause { "WHERE " } else { "and " });

        match (before, after) {
            (Some(_), Some(_)) => query.push_str("id > ? and id < ? "), // after, before
            (Some(_), _) => query.push_str("id < ? "),                  // before
            (_, Some(_)) => query.push_str("id > ? "),                  // after
            _ => unreachable!(),
        }
    }
}

pub fn connections_append_query_string_order(
    query: &mut String, first: Option<usize>, last: Option<usize>,
) {
    if first.is_some() {
        query.push_str("ORDER BY id ASC LIMIT ? "); // first
    } else if last.is_some() {
        query.push_str("ORDER BY id DESC LIMIT ? "); // last
    }
}

pub fn connections_append_query_string(
    query: &mut String, has_where_clause: bool, after: Option<u32>, before: Option<u32>,
    first: Option<usize>, last: Option<usize>,
) {
    connections_append_query_string_page(query, has_where_clause, after, before);
    connections_append_query_string_order(query, first, last);
}

pub type SqlQuery<'q> = sqlx::query::Query<
    'q,
    mysql::MySql,
    <mysql::MySql as sqlx::database::HasArguments<'q>>::Arguments,
>;

pub fn connections_bind_query_parameters_page(
    mut query: SqlQuery, after: Option<u32>, before: Option<u32>,
) -> SqlQuery {
    match (before, after) {
        (Some(before), Some(after)) => query = query.bind(before).bind(after),
        (Some(before), _) => query = query.bind(before),
        (_, Some(after)) => query = query.bind(after),
        _ => {}
    }
    query
}

pub fn connections_bind_query_parameters_order(
    mut query: SqlQuery, first: Option<usize>, last: Option<usize>,
) -> SqlQuery {
    // Actual limits are N+1 to check if previous/next pages
    if let Some(first) = first {
        query = query.bind(first as u32 + 1);
    } else if let Some(last) = last {
        query = query.bind(last as u32 + 1);
    }

    query
}

pub fn connections_bind_query_parameters(
    mut query: SqlQuery, after: Option<u32>, before: Option<u32>, first: Option<usize>,
    last: Option<usize>,
) -> SqlQuery {
    query = connections_bind_query_parameters_page(query, after, before);
    query = connections_bind_query_parameters_order(query, first, last);
    query
}

pub fn connections_pages_info(
    results_count: usize, first: Option<usize>, last: Option<usize>,
) -> (bool, bool) {
    let mut has_previous_page = false;
    let mut has_next_page = false;

    if let Some(first) = first {
        if results_count == first + 1 {
            has_next_page = true;
        }
    }

    if let Some(last) = last {
        if results_count == last + 1 {
            has_previous_page = true;
        }
    }

    (has_previous_page, has_next_page)
}

#[derive(async_graphql::Interface)]
#[graphql(field(name = "id", type = "ID"))]
pub enum Node {
    Map(Map),
    Player(Player),
}

#[async_graphql::Object]
impl Player {
    async fn id(&self) -> async_graphql::ID {
        async_graphql::ID(format!("v0:Player:{}", self.id))
    }

    async fn login(&self) -> String {
        self.login.clone()
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn maps(
        &self, ctx: &async_graphql::Context<'_>, after: Option<String>, before: Option<String>,
        first: Option<i32>, last: Option<i32>,
    ) -> async_graphql::Result<connection::Connection<ID, Map>> {
        connection::query(
            after,
            before,
            first,
            last,
            |after: Option<ID>, before: Option<ID>, first: Option<usize>, last: Option<usize>| async move {
                let after = decode_id(after.as_ref());
                let before = decode_id(before.as_ref());

                // Build the query string
                let mut query = String::from("SELECT id, game_id, player_id, name FROM maps m1 WHERE player_id = ? ");
                connections_append_query_string_page(&mut query, true, after, before);
                query.push_str(" GROUP BY name HAVING id = (SELECT MAX(id) FROM maps m2 WHERE player_id = ? AND m1.name = m2.name)");
                connections_append_query_string_order(&mut query, first, last);

                let reversed = first.is_none() && last.is_some();

                // Bind the parameters
                let mut query = sqlx::query(&query);
                query = query.bind(self.id);
                query = connections_bind_query_parameters_page(query, after, before);
                query = query.bind(self.id);
                query = connections_bind_query_parameters_order(query, first, last);

                // Execute the query
                let mysql_pool = ctx.data_unchecked::<MySqlPool>();
                let mut maps =
                    query
                        .map(|x: mysql::MySqlRow| {
                            let cursor = ID(format!("v0:Map:{}", x.get::<u32, _>(0)));
                            connection::Edge::new(cursor, Map::from_row(&x).unwrap())
                        })
                        .fetch_all(mysql_pool)
                        .await?;

                if reversed {
                    maps.reverse();
                }

                let (has_previous_page, has_next_page) = connections_pages_info(maps.len(), first, last);
                let mut connection = connection::Connection::new(has_previous_page, has_next_page);
                connection.edges.extend(maps);
                Ok::<_, sqlx::Error>(connection)
            },
        )
        .await
    }

    async fn records(
        &self, ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<crate::database::Database>();
        let redis_pool = ctx.data_unchecked::<crate::database::RedisPool>();
        let mysql_pool = ctx.data_unchecked::<crate::database::MySqlPool>();
        let mut redis_conn = redis_pool.get().await.unwrap();

        // Query the records with these ids
        let query = sqlx::query_as!(
            Record,
            "SELECT * FROM records WHERE player_id = ? ORDER BY updated_at DESC LIMIT 100",
            self.id
        );

        let mut records = query
            .map(|record: Record| RankedRecord { rank: 0, record })
            .fetch_all(mysql_pool)
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();

        for mut record in &mut records {
            let map_game_id = sqlx::query_scalar!(
                "SELECT game_id from maps where id = ?",
                record.record.map_id
            )
            .fetch_one(&db.mysql_pool)
            .await?;
            let key = format!("l0:{}", map_game_id);
            let mut player_rank: Option<i64> = redis_conn.zrank(&key, self.id).await.unwrap();
            if player_rank.is_none() {
                crate::update_redis_leaderboard(db, &key, record.record.map_id).await?;
                player_rank = redis_conn.zrank(&key, self.id).await.unwrap();
            }

            record.rank = player_rank.map(|r: i64| (r as u64) as i32).unwrap_or(-1) + 1;
        }

        Ok(records)
    }
}

#[async_graphql::Object]
impl Map {
    async fn id(&self) -> async_graphql::ID {
        async_graphql::ID(format!("v0:Map:{}", self.id))
    }

    async fn game_id(&self) -> String {
        self.game_id.clone()
    }

    async fn player_id(&self) -> async_graphql::ID {
        async_graphql::ID(format!("v0:Player:{}", self.player_id))
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn records(
        &self, ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<crate::database::Database>();
        let redis_pool = ctx.data_unchecked::<crate::database::RedisPool>();
        let mysql_pool = ctx.data_unchecked::<crate::database::MySqlPool>();
        let mut redis_conn = redis_pool.get().await.unwrap();

        let key = format!("l0:{}", self.game_id);

        crate::update_redis_leaderboard(db, &key, self.id).await?;

        let record_ids: Vec<i32> = redis_conn.zrange(key, 0, 99).await.unwrap();
        let player_cond = if record_ids.is_empty() {
            "".to_string()
        } else {
            format!(
                "AND player_id IN ({})",
                record_ids
                    .iter()
                    .map(|_| "?".to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            )
        };

        // Query the records with these ids
        let query = format!(
            "SELECT * FROM records WHERE map_id = ? {} ORDER BY time ASC",
            player_cond
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

#[async_graphql::Object]
impl RankedRecord {
    async fn rank(&self) -> i32 {
        self.rank
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<dataloader::DataLoader<MapLoader>>()
            .load_one(self.record.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.record.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))
    }

    async fn time(&self) -> i32 {
        self.record.time
    }

    async fn respawn_count(&self) -> i32 {
        self.record.respawn_count
    }

    async fn try_count(&self) -> i32 {
        self.record.respawn_count
    }

    async fn created_at(&self) -> chrono::NaiveDateTime {
        self.record.created_at
    }

    async fn updated_at(&self) -> chrono::NaiveDateTime {
        self.record.updated_at
    }

    async fn flags(&self) -> u32 {
        self.record.flags
    }
}

pub struct PlayerLoader(crate::Database);

impl PlayerLoader {
    pub fn new(db: crate::Database) -> Self {
        Self(db)
    }
}

#[async_graphql::async_trait::async_trait]
impl async_graphql::dataloader::Loader<u32> for PlayerLoader {
    type Value = Player;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, Self::Value>, Self::Error> {
        let query = format!(
            "SELECT * FROM players WHERE id IN ({})",
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
                let player = Player::from_row(&row).unwrap();
                (player.id, player)
            })
            .fetch_all(&self.0.mysql_pool)
            .await?
            .into_iter()
            .collect::<HashMap<_, _>>())
    }
}

pub struct MapLoader(MySqlPool);

impl MapLoader {
    pub fn new(mysql_pool: MySqlPool) -> Self {
        Self(mysql_pool)
    }
}

#[async_graphql::async_trait::async_trait]
impl async_graphql::dataloader::Loader<u32> for MapLoader {
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
            .collect::<HashMap<_, _>>())
    }
}
