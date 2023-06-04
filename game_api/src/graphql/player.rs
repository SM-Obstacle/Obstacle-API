use std::{collections::HashMap, sync::Arc};

use async_graphql::{connection, dataloader::Loader, Context, ID};
use deadpool_redis::Pool as RedisPool;
use sqlx::{mysql, FromRow, MySqlPool, Row};

use crate::{
    models::{Banishment, Map, Player, RankedRecord, Record, Role},
    redis,
    utils::format_map_key,
    Database,
};

use super::utils::{
    connections_append_query_string_order, connections_append_query_string_page,
    connections_bind_query_parameters_order, connections_bind_query_parameters_page,
    connections_pages_info, decode_id, get_rank_of,
};

#[async_graphql::Object]
impl Player {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Player:{}", self.id))
    }

    async fn login(&self) -> String {
        self.login.clone()
    }

    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn banishments(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(
            sqlx::query_as("SELECT * FROM banishments WHERE player_id = ?")
                .bind(self.id)
                .fetch_all(db)
                .await?,
        )
    }

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Role> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(sqlx::query_as("SELECT * FROM role WHERE id = ?")
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
        let db = ctx.data_unchecked::<Database>();
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mut redis_conn = redis_pool.get().await?;
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();

        // Query the records with these ids
        let records = sqlx::query_as!(
            Record,
            "SELECT * FROM records r
            WHERE player_id = ? AND id = (
                SELECT MAX(id) FROM records
                WHERE player_id = ? AND map_id = r.map_id
            )
            ORDER BY id DESC
            LIMIT 100",
            self.id,
            self.id
        )
        .fetch_all(mysql_pool)
        .await?;

        let mut ranked_records = Vec::with_capacity(records.len());

        for record in records {
            let key = format_map_key(record.map_id);

            let rank = match get_rank_of(&mut redis_conn, &key, record.time).await? {
                Some(rank) => rank,
                None => {
                    redis::update_leaderboard(&db, &key, record.map_id).await?;
                    get_rank_of(&mut redis_conn, &key, record.time)
                        .await?
                        .expect(&format!(
                            "redis leaderboard for (`{key}`) should be updated at this point"
                        ))
                }
            };

            ranked_records.push(RankedRecord { rank, record });
        }

        Ok(ranked_records)
    }
}

pub struct PlayerLoader(pub MySqlPool);

#[async_graphql::async_trait::async_trait]
impl Loader<u32> for PlayerLoader {
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
