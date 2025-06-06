use std::{collections::HashMap, sync::Arc};

use async_graphql::{Enum, ID, connection, dataloader::Loader};
use records_lib::{
    Database, DatabaseConnection, MySqlConnection, RedisConnection, acquire,
    models::{self, Role},
    opt_event::OptEvent,
    transaction::{self, ReadOnly, TxnGuard},
};
use sqlx::{FromRow, MySqlPool, Row, mysql};

use crate::RecordsErrorKind;

use super::{
    SortState,
    ban::Banishment,
    get_rank,
    map::Map,
    record::RankedRecord,
    utils::{
        connections_append_query_string_order, connections_append_query_string_page,
        connections_bind_query_parameters_order, connections_bind_query_parameters_page,
        connections_pages_info, decode_id,
    },
};

#[derive(Copy, Clone, Eq, PartialEq, Enum)]
#[repr(u8)]
enum PlayerRole {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl TryFrom<Role> for PlayerRole {
    type Error = RecordsErrorKind;

    fn try_from(role: Role) -> Result<Self, Self::Error> {
        if role.id < 3 {
            // SAFETY: enum is repr(u8) and role id is in range
            Ok(unsafe { std::mem::transmute::<u8, PlayerRole>(role.id) })
        } else {
            Err(RecordsErrorKind::UnknownRole(role.id, role.role_name))
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Player {
    #[sqlx(flatten)]
    pub inner: models::Player,
}

impl From<models::Player> for Player {
    fn from(inner: models::Player) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Player {
    pub async fn id(&self) -> ID {
        ID(format!("v0:Player:{}", self.inner.id))
    }

    async fn login(&self) -> &str {
        &self.inner.login
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn zone_path(&self) -> Option<&str> {
        self.inner.zone_path.as_deref()
    }

    async fn banishments(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(
            sqlx::query_as("SELECT * FROM banishments WHERE player_id = ?")
                .bind(self.inner.id)
                .fetch_all(db)
                .await?,
        )
    }

    async fn role(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<PlayerRole> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let r: Role = sqlx::query_as("SELECT * FROM role WHERE id = ?")
            .bind(self.inner.role)
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
                query = query.bind(self.inner.id);
                query = connections_bind_query_parameters_page(query, after, before);
                query = query.bind(self.inner.id);
                query = connections_bind_query_parameters_order(query, first, last);

                // Execute the query
                let mysql_pool = ctx.data_unchecked::<MySqlPool>();
                let mut maps =
                    query
                        .map(|x: mysql::MySqlRow| {
                            let cursor = ID(format!("v0:Map:{}", x.get::<u32, _>(0)));
                            connection::Edge::new(cursor, models::Map::from_row(&x).unwrap().into())
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
        let conn = acquire!(db?);

        records_lib::assert_future_send(transaction::within(
            conn.mysql_conn,
            ReadOnly,
            async |mysql_conn, guard| {
                get_player_records(
                    mysql_conn,
                    conn.redis_conn,
                    guard,
                    self.inner.id,
                    Default::default(),
                    date_sort_by,
                )
                .await
            },
        ))
        .await
    }
}

async fn get_player_records<M>(
    mysql_conn: MySqlConnection<'_>,
    redis_conn: &mut RedisConnection,
    guard: TxnGuard<'_, M>,
    player_id: u32,
    event: OptEvent<'_>,
    date_sort_by: Option<SortState>,
) -> async_graphql::Result<Vec<RankedRecord>> {
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn,
    };

    let date_sort_by = SortState::sql_order_by(&date_sort_by);

    // Query the records with these ids

    let query = format!(
        "SELECT * FROM global_records r
            WHERE record_player_id = ?
            ORDER BY record_date {date_sort_by}
            LIMIT 100",
    );

    let records = sqlx::query_as::<_, models::Record>(&query)
        .bind(player_id)
        .fetch_all(&mut **conn.mysql_conn)
        .await?;

    let mut ranked_records = Vec::with_capacity(records.len());

    for record in records {
        let rank = get_rank(
            &mut conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
            guard,
        )
        .await?;

        ranked_records.push(models::RankedRecord { rank, record }.into());
    }

    Ok(ranked_records)
}

pub struct PlayerLoader(pub MySqlPool);

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
                (player.inner.id, player)
            })
            .fetch_all(&self.0)
            .await?
            .into_iter()
            .collect())
    }
}
