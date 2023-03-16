use async_graphql::{connection, dataloader, ID};
use deadpool_redis::redis::AsyncCommands;
use sqlx::{mysql, query_as, query_scalar, FromRow, MySqlPool, Row};
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
    query: &mut String,
    has_where_clause: bool,
    after: Option<u32>,
    before: Option<u32>,
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
    query: &mut String,
    first: Option<usize>,
    last: Option<usize>,
) {
    if first.is_some() {
        query.push_str("ORDER BY id ASC LIMIT ? "); // first
    } else if last.is_some() {
        query.push_str("ORDER BY id DESC LIMIT ? "); // last
    }
}

pub fn connections_append_query_string(
    query: &mut String,
    has_where_clause: bool,
    after: Option<u32>,
    before: Option<u32>,
    first: Option<usize>,
    last: Option<usize>,
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
    mut query: SqlQuery,
    after: Option<u32>,
    before: Option<u32>,
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
    mut query: SqlQuery,
    first: Option<usize>,
    last: Option<usize>,
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
    mut query: SqlQuery,
    after: Option<u32>,
    before: Option<u32>,
    first: Option<usize>,
    last: Option<usize>,
) -> SqlQuery {
    query = connections_bind_query_parameters_page(query, after, before);
    query = connections_bind_query_parameters_order(query, first, last);
    query
}

pub fn connections_pages_info(
    results_count: usize,
    first: Option<usize>,
    last: Option<usize>,
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
    async fn id(&self) -> ID {
        ID(format!("v0:Player:{}", self.id))
    }

    async fn login(&self) -> String {
        self.login.clone()
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn banishments(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(query_as("SELECT * FROM banishments WHERE player_id = ?")
            .bind(self.id)
            .fetch_all(db)
            .await?)
    }

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Role> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(query_as("SELECT * FROM role WHERE id = ?")
            .bind(self.role)
            .fetch_one(db)
            .await?)
    }

    async fn maps(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
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
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<crate::database::Database>();
        let redis_pool = ctx.data_unchecked::<crate::database::RedisPool>();
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let mut redis_conn = redis_pool.get().await.unwrap();

        // Query the records with these ids
        let query = query_as!(
            Record,
            "SELECT * FROM records WHERE player_id = ? ORDER BY record_date DESC LIMIT 100",
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
    async fn id(&self) -> ID {
        ID(format!("v0:Map:{}", self.id))
    }

    async fn game_id(&self) -> String {
        self.game_id.clone()
    }

    async fn player_id(&self) -> ID {
        ID(format!("v0:Player:{}", self.player_id))
    }

    async fn cps_number(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<i64> {
        Ok(ctx
            .data_unchecked::<dataloader::DataLoader<CpsNumberLoader>>()
            .load_one(self.id)
            .await?
            .unwrap_or(0))
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

    async fn medal_for(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<Option<MedalPrice>> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let player_id = query_scalar!("SELECT id FROM players WHERE login = ?", login)
            .fetch_one(db)
            .await?;

        Ok(
            query_as("SELECT * FROM prizes WHERE map_id = ? AND player_id = ? ORDER BY medal DESC")
                .bind(self.id)
                .bind(player_id)
                .fetch_optional(db)
                .await?,
        )
    }

    async fn ratings(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Rating>> {
        let db = ctx.data_unchecked();
        Ok(query_as("SELECT * FROM rating WHERE map_id = ?")
            .bind(self.id)
            .fetch_all(db)
            .await?)
    }

    async fn average_rating(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<PlayerRating>> {
        let db = ctx.data_unchecked();
        let fetch_all = query_as(
            r#"SELECT CAST(0 AS UNSIGNED) AS "player_id", map_id, kind,
            AVG(rating) AS "rating" FROM player_rating
            WHERE map_id = ? GROUP BY kind ORDER BY kind"#,
        )
        .bind(self.id)
        .fetch_all(db)
        .await?;
        println!("{fetch_all:?}");
        Ok(fetch_all)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<crate::database::Database>();
        let redis_pool = ctx.data_unchecked::<crate::database::RedisPool>();
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
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

    async fn cps_times(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<CheckpointTimes>> {
        let db = &ctx.data_unchecked::<crate::database::Database>().mysql_pool;

        Ok(query_as!(
            CheckpointTimes,
            "SELECT * FROM checkpoint_times WHERE record_id = ? AND map_id = ? ORDER BY cp_num",
            self.record.id,
            self.record.map_id,
        )
        .fetch_all(db)
        .await?)
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

    async fn record_date(&self) -> chrono::NaiveDateTime {
        self.record.record_date
    }

    async fn flags(&self) -> u32 {
        self.record.flags
    }
}

#[async_graphql::Object]
impl Banishment {
    async fn id(&self) -> u32 {
        self.inner.id
    }

    async fn date_ban(&self) -> chrono::NaiveDateTime {
        self.inner.date_ban
    }

    async fn duration(&self) -> Option<u32> {
        self.inner.duration
    }

    async fn was_reprieved(&self) -> bool {
        self.was_reprieved
    }

    async fn reason(&self) -> Option<&str> {
        self.inner.reason.as_deref()
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.inner.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Bannished player not found."))
    }

    async fn banished_by(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.inner.banished_by)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Bannisher player not found."))
    }
}

#[async_graphql::Object]
impl MedalPrice {
    async fn id(&self) -> u32 {
        self.id
    }

    async fn price_date(&self) -> chrono::NaiveDateTime {
        self.price_date
    }

    async fn medal(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Medal> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(query_as("SELECT * FROM medal_type WHERE id = ?")
            .bind(self.medal)
            .fetch_one(db)
            .await?)
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<dataloader::DataLoader<MapLoader>>()
            .load_one(self.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }
}

#[async_graphql::Object]
impl Rating {
    async fn ratings(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<PlayerRating>> {
        let db = ctx.data_unchecked();
        Ok(
            query_as("SELECT * FROM player_rating WHERE player_id = ? AND map_id = ?")
                .bind(self.player_id)
                .bind(self.map_id)
                .fetch_all(db)
                .await?,
        )
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Player> {
        ctx.data_unchecked::<dataloader::DataLoader<PlayerLoader>>()
            .load_one(self.player_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Map> {
        ctx.data_unchecked::<dataloader::DataLoader<MapLoader>>()
            .load_one(self.map_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Voter player not found."))
    }

    async fn rating_date(&self) -> chrono::NaiveDateTime {
        self.rating_date
    }

    async fn denominator(&self) -> u8 {
        self.denominator
    }
}

#[async_graphql::Object]
impl PlayerRating {
    async fn kind(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<RatingKind> {
        let db = ctx.data_unchecked();
        Ok(query_as("SELECT * FROM rating_kind WHERE id = ?")
            .bind(self.kind)
            .fetch_one(db)
            .await?)
    }

    async fn rating(&self) -> f32 {
        self.rating
    }
}

pub struct PlayerLoader(pub MySqlPool);

#[async_graphql::async_trait::async_trait]
impl dataloader::Loader<u32> for PlayerLoader {
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
            .fetch_all(&self.0)
            .await?
            .into_iter()
            .collect())
    }
}

pub struct MapLoader(pub MySqlPool);

#[async_graphql::async_trait::async_trait]
impl dataloader::Loader<u32> for MapLoader {
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
impl dataloader::Loader<u32> for CpsNumberLoader {
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
