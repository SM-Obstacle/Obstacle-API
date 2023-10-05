use std::{collections::HashMap, sync::Arc};

use async_graphql::{connection, dataloader::Loader, Context, Enum, ID};
use deadpool_redis::Pool as RedisPool;
use futures::StreamExt;
use sqlx::{mysql, FromRow, MySqlPool, Row};

use crate::{
    models::{Banishment, Map, Player, RankedRecord, Role},
    utils::format_map_key,
    Database, RecordsError,
};

use super::{
    get_rank_or_full_update,
    utils::{
        connections_append_query_string_order, connections_append_query_string_page,
        connections_bind_query_parameters_order, connections_bind_query_parameters_page,
        connections_pages_info, decode_id, RecordAttr,
    },
    SortState,
};

#[derive(Copy, Clone, Eq, PartialEq, Enum)]
#[repr(u8)]
enum PlayerRole {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl TryFrom<Role> for PlayerRole {
    type Error = RecordsError;

    fn try_from(role: Role) -> Result<Self, Self::Error> {
        if role.id < 3 {
            // SAFETY: enum is repr(u8) and role id is in range
            Ok(unsafe { std::mem::transmute(role.id) })
        } else {
            Err(RecordsError::UnknownRole(role.id, role.role_name))
        }
    }
}

#[async_graphql::Object]
impl Player {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Player:{}", self.id))
    }

    async fn login(&self) -> &str {
        &self.login
    }

    async fn name(&self) -> &str {
        &self.name
    }

    async fn zone_path(&self) -> Option<&str> {
        self.zone_path.as_deref()
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

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<PlayerRole> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let r: Role = sqlx::query_as("SELECT * FROM role WHERE id = ?")
            .bind(self.role)
            .fetch_one(db)
            .await?;

        Ok(r.try_into()?)
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
                let mut query = String::from("SELECT m1.* FROM maps m1 WHERE player_id = ? ");
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
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mut redis_conn = redis_pool.get().await?;
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();

        let date_sort_by = SortState::sql_order_by(&date_sort_by);

        // Query the records with these ids

        let query = format!(
            "SELECT r.*, m.reversed AS reversed
            FROM records r
            INNER JOIN maps m ON m.id = r.map_id
            INNER JOIN (
                SELECT IF(m.reversed, MAX(time), MIN(time)) AS time, map_id
                FROM records r
                INNER JOIN maps m ON m.id = r.map_id
                WHERE r.player_id = ?
                GROUP BY map_id
            ) t ON t.time = r.time AND t.map_id = r.map_id
            WHERE r.player_id = ? AND m.game_id NOT LIKE '%_benchmark'
            ORDER BY record_date {date_sort_by}
            LIMIT 100",
        );

        let mut records = sqlx::query_as::<_, RecordAttr>(&query)
            .bind(self.id)
            .bind(self.id)
            .fetch(mysql_pool);

        let mut ranked_records = Vec::with_capacity(records.size_hint().0);

        while let Some(record) = records.next().await {
            let RecordAttr { record, reversed } = record?;

            let key = format_map_key(record.map_id);

            let rank = get_rank_or_full_update(
                db,
                &mut redis_conn,
                &key,
                record.map_id,
                record.time,
                reversed.unwrap_or(false),
            )
            .await?;

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
