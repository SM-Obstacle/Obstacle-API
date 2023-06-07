use actix_web::web::{self, Data};
use actix_web::{HttpResponse, Resource, Responder};
use async_graphql::dataloader::DataLoader;
use async_graphql::extensions::ApolloTracing;
use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql::{connection, ID};
use async_graphql_actix_web::GraphQLRequest;
use sqlx::{mysql, query_as, FromRow, MySqlPool, Row};
use std::vec::Vec;

use deadpool_redis::Pool as RedisPool;

use crate::auth::{self, AuthHeader};
use crate::graphql::map::MapLoader;
use crate::graphql::player::PlayerLoader;
use crate::models::{Banishment, Event, Map, Player, RankedRecord, Record, Role};
use crate::utils::format_map_key;
use crate::{redis, Database};

use self::event::{EventCategoryLoader, EventLoader};
use self::utils::{
    connections_append_query_string, connections_bind_query_parameters, connections_pages_info,
    decode_id, get_rank_of,
};

mod ban;
mod event;
mod map;
mod medal;
mod player;
mod rating;
mod record;
mod utils;

#[derive(async_graphql::Interface)]
#[graphql(field(name = "id", type = "ID"))]
pub enum Node {
    Map(Map),
    Player(Player),
}

pub struct QueryRoot;

#[async_graphql::Object]
impl QueryRoot {
    async fn banishments(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked();
        let Some(auth_header) = ctx.data_unchecked::<Option<AuthHeader>>() else {
            return Err(async_graphql::Error::new("Forbidden"));
        };
        auth::check_auth_for(db, auth_header.clone(), Role::Admin).await?;
        Ok(query_as("SELECT * FROM banishments")
            .fetch_all(&db.mysql_pool)
            .await?)
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Event>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        Ok(query_as("SELECT * FROM event").fetch_all(db).await?)
    }

    async fn players(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> async_graphql::Result<connection::Connection<ID, Player>> {
        connection::query(
            after,
            before,
            first,
            last,
            |after: Option<ID>, before: Option<ID>, first: Option<usize>, last: Option<usize>| async move {
                let after = decode_id(after.as_ref());
                let before = decode_id(before.as_ref());

                // Build the query string
                let mut query = String::from("SELECT id, login, name FROM players ");
                connections_append_query_string(&mut query, false, after, before, first, last);
                let reversed = first.is_none() && last.is_some();

                // Bind the parameters
                let mut query = sqlx::query(&query);
                query = connections_bind_query_parameters(query, after, before, first, last);

                // Execute the query
                let mysql_pool = ctx.data_unchecked::<MySqlPool>();
                let mut players =
                    query
                        .map(|x: mysql::MySqlRow| {
                            let cursor = ID(format!("v0:Player:{}", x.get::<u32, _>(0)));
                            connection::Edge::new(cursor, Player::from_row(&x).unwrap())
                        })
                        .fetch_all(mysql_pool)
                        .await?;
                if reversed {
                    players.reverse();
                }

                let (has_previous_page, has_next_page) = connections_pages_info(players.len(), first, last);
                let mut connection = connection::Connection::new(has_previous_page, has_next_page);
                connection.edges.extend(players);

                Ok::<_, sqlx::Error>(connection)
            },
        )
        .await
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
                let time_start = std::time::SystemTime::now();

                let after  = decode_id(after.as_ref());
                let before = decode_id(before.as_ref());

                // Build the query string
                let mut query = String::from("SELECT id, game_id, player_id, name FROM maps ");
                connections_append_query_string(&mut query, false, after, before, first, last);

                println!("root maps: {:.3?}\n", time_start.elapsed().unwrap());

                let reversed = first.is_none() && last.is_some();

                // Bind the parameters
                let mut query = sqlx::query(&query);
                query = connections_bind_query_parameters(query, after, before, first, last);

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

                println!("root maps: done await query {:.3?}", time_start.elapsed().unwrap());

                if reversed {
                    maps.reverse();
                }

                let (has_previous_page, has_next_page) = connections_pages_info(maps.len(), first, last);
                let mut connection = connection::Connection::new(has_previous_page, has_next_page);
                connection.edges.extend(maps);

                println!("root maps: done connection {:.3?}", time_start.elapsed().unwrap());
                Ok::<_, sqlx::Error>(connection)
            },
        )
        .await
    }

    // Global unique identifiers
    async fn node(&self, ctx: &async_graphql::Context<'_>, id: async_graphql::ID) -> Option<Node> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let parts: Vec<&str> = id.split(':').collect();
        println!("node");

        if parts.len() != 3 || parts[0] != "v0" || (parts[1] != "Map" && parts[1] != "Player") {
            println!(
                "invalid, len: {}, [0]: {}, [1]: {}",
                parts.len(),
                parts[0],
                parts[1]
            );
            None
        } else if parts[1] == "Map" {
            let id = parts[2].parse::<u32>().ok()?;
            let query = "SELECT id, game_id, player_id, name FROM maps WHERE id = ? ";
            println!("{} _ id: {}", &query, id);
            let query = sqlx::query(query).bind(id);
            let result = query.fetch_one(mysql_pool).await.ok()?;
            Some(Node::Map(Map::from_row(&result).unwrap()))
        } else {
            let id = parts[2].parse::<u32>().ok()?;
            let query = "SELECT id, login, name FROM players WHERE id = ? ";
            println!("{} _ id: {}", &query, id);
            let query = sqlx::query(query).bind(id);
            let result = query.fetch_one(mysql_pool).await.ok()?;
            Some(Node::Player(Player::from_row(&result).unwrap()))
        }
    }

    // Old website

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
        game_id: String,
    ) -> async_graphql::Result<Map> {
        let db = ctx.data_unchecked::<Database>();
        crate::http::player::get_map_from_game_id(db, &game_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
    }

    async fn player(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<Player> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let query =
            sqlx::query_as::<_, Player>("SELECT * FROM players WHERE login = ? ").bind(login);
        Ok(query.fetch_one(mysql_pool).await?)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let redis_pool = ctx.data_unchecked::<RedisPool>();
        let mut redis_conn = redis_pool.get().await?;
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();

        let records = sqlx::query_as::<_, Record>(
            "SELECT * FROM records r
            WHERE id = (
                SELECT MAX(id) FROM records
                WHERE player_id = r.player_id AND map_id = r.map_id
            )
            ORDER BY id DESC
            LIMIT 100",
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

pub type Schema = async_graphql::Schema<
    QueryRoot,
    async_graphql::EmptyMutation,
    async_graphql::EmptySubscription,
>;

fn create_schema(db: Database) -> Schema {
    let schema = async_graphql::Schema::build(
        QueryRoot,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .extension(ApolloTracing)
    .data(DataLoader::new(
        PlayerLoader(db.mysql_pool.clone()),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        MapLoader(db.mysql_pool.clone()),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        EventLoader(db.mysql_pool.clone()),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        EventCategoryLoader(db.mysql_pool.clone()),
        tokio::spawn,
    ))
    .data(db.mysql_pool.clone())
    .data(db.redis_pool.clone())
    .data(db)
    .limit_depth(16)
    .finish();

    #[cfg(feature = "output_gql_schema")]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!(
                "schemas/schema_{}.graphql",
                chrono::Utc::now().timestamp()
            ))
            .unwrap()
            .write_all(schema.sdl().as_bytes())
            .unwrap();
    }

    #[cfg(feature = "localhost_test")]
    {
        println!("----------- Schema:");
        println!("{}", &schema.sdl());
    }

    schema
}

async fn index_graphql(
    auth: Option<AuthHeader>,
    schema: Data<Schema>,
    request: GraphQLRequest,
) -> impl Responder {
    web::Json(schema.execute(request.into_inner().data(auth)).await)
}

async fn index_playground() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(playground_source(GraphQLPlaygroundConfig::new("/graphql")))
}

pub fn graphql_route(db: Database) -> Resource {
    web::resource("/graphql")
        .app_data(Data::new(create_schema(db)))
        .route(web::get().to(index_playground))
        .route(web::post().to(index_graphql))
}
