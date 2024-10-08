use actix_session::Session;
use actix_web::web::{self, Data};
use actix_web::{HttpResponse, Resource, Responder};
use async_graphql::dataloader::DataLoader;
use async_graphql::extensions::ApolloTracing;
use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql::{connection, Enum, ErrorExtensionValues, Object, Value, ID};
use async_graphql_actix_web::GraphQLRequest;
use futures::StreamExt as _;
use records_lib::models;
use records_lib::update_ranks::get_rank;
use records_lib::{must, Database};
use reqwest::Client;
use sqlx::{mysql, query_as, FromRow, MySqlPool, Row};
use std::vec::Vec;
use tracing_actix_web::RequestId;

use crate::auth::{self, privilege, WebToken, WEB_TOKEN_SESS_KEY};
use crate::graphql::map::MapLoader;
use crate::graphql::player::PlayerLoader;

use self::ban::Banishment;
use self::event::{Event, EventCategoryLoader, EventEdition, EventLoader};
use self::map::Map;
use self::mappack::Mappack;
use self::player::Player;
use self::record::RankedRecord;
use self::utils::{
    connections_append_query_string, connections_bind_query_parameters, connections_pages_info,
    decode_id,
};

mod ban;
mod event;
mod map;
mod mappack;
mod player;
mod rating;
mod record;
mod utils;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum SortState {
    Sort,
    Reverse,
}

impl SortState {
    pub fn sql_order_by(this: &Option<Self>) -> &'static str {
        match this.as_ref() {
            Some(Self::Reverse) => "ASC",
            _ => "DESC",
        }
    }
}

#[derive(async_graphql::Interface)]
#[graphql(field(name = "id", ty = "ID"))]
enum Node {
    Map(Map),
    Player(Player),
}

#[repr(transparent)]
struct Article {
    inner: models::Article,
}

impl From<models::Article> for Article {
    fn from(inner: models::Article) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl Article {
    async fn date(&self) -> chrono::DateTime<chrono::Utc> {
        self.inner.article_date
    }

    async fn authors(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Player>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let players = sqlx::query_as(
            "select p.* from players p
            inner join article_authors aa on aa.author_id = p.id
            where aa.article_id = ?",
        )
        .bind(self.inner.id)
        .fetch_all(db)
        .await?;
        Ok(players)
    }

    async fn content(&self) -> async_graphql::Result<String> {
        let content = tokio::fs::read_to_string(&self.inner.path).await?;
        Ok(content)
    }
}

struct QueryRoot;

#[async_graphql::Object]
impl QueryRoot {
    async fn latest_news(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Option<Article>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let article = sqlx::query_as(
            "select * from article
            where hide is null or hide = 0
            order by article_date desc
            limit 1",
        )
        .fetch_optional(db)
        .await?;
        Ok(article.map(From::<models::Article>::from))
    }

    async fn article(
        &self,
        ctx: &async_graphql::Context<'_>,
        slug: String,
    ) -> async_graphql::Result<Option<Article>> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let article = sqlx::query_as(
            "select * from article
            where (hide is null or hide = 0) and slug = ?",
        )
        .bind(slug)
        .fetch_optional(db)
        .await?;
        Ok(article.map(From::<models::Article>::from))
    }

    async fn resources_content(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<models::ResourcesContent> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();
        let txt = sqlx::query_as(
            "SELECT content, created_at AS last_modified
            FROM resources_content
            ORDER BY created_at DESC
            LIMIT 1",
        )
        .fetch_one(mysql_pool)
        .await?;
        Ok(txt)
    }

    async fn event_edition_from_mx_id(
        &self,
        ctx: &async_graphql::Context<'_>,
        mx_id: i64,
    ) -> async_graphql::Result<Option<EventEdition>> {
        let mysql_pool = ctx.data_unchecked::<MySqlPool>();

        let edition = sqlx::query_as::<_, models::EventEdition>(
            "SELECT * FROM event_edition WHERE mx_id = ?",
        )
        .bind(mx_id)
        .fetch_optional(mysql_pool)
        .await?;

        Ok(match edition {
            Some(edition) => Some(EventEdition::from_inner(edition, mysql_pool).await?),
            None => None,
        })
    }

    async fn mappack(
        &self,
        ctx: &async_graphql::Context<'_>,
        mappack_id: String,
    ) -> async_graphql::Result<Mappack> {
        let res = mappack::get_mappack(ctx, mappack_id).await?;
        Ok(res)
    }

    async fn banishments(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<Banishment>> {
        let db = ctx.data_unchecked();
        let Some(WebToken { login, token }) = ctx.data_opt::<WebToken>() else {
            return Err(async_graphql::Error::new("Unauthorized"));
        };
        auth::website_check_auth_for(db, login, token, privilege::ADMIN).await?;
        Ok(query_as("SELECT * FROM banishments")
            .fetch_all(&db.mysql_pool)
            .await?)
    }

    async fn event(
        &self,
        ctx: &async_graphql::Context<'_>,
        handle: String,
    ) -> async_graphql::Result<Event> {
        let db = ctx.data_unchecked::<MySqlPool>();

        let mut conn = db.acquire().await?;
        let event = must::have_event_handle(&mut conn, &handle).await?;
        conn.close().await?;

        Ok(event.into())
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Event>> {
        let db = ctx.data_unchecked::<MySqlPool>();

        Ok(query_as(
            "select e.* from event e
            inner join event_edition ee on ee.event_id = e.id
            where ee.start_date < sysdate()
                and (ee.ttl is null or sysdate() < ee.start_date + interval ee.ttl second)
            group by e.id",
        )
        .fetch_all(db)
        .await?)
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

    async fn record(
        &self,
        ctx: &async_graphql::Context<'_>,
        record_id: u32,
    ) -> async_graphql::Result<RankedRecord> {
        let db = ctx.data_unchecked::<Database>();
        let mut conn = db.acquire().await?;

        let Some(record) =
            sqlx::query_as::<_, models::Record>("select * from records where record_id = ?")
                .bind(record_id)
                .fetch_optional(&db.mysql_pool)
                .await?
        else {
            return Err(async_graphql::Error::new("Record not found."));
        };

        let out = models::RankedRecord {
            rank: get_rank(
                &mut conn,
                record.map_id,
                record.record_player_id,
                record.time,
                Default::default(),
            )
            .await?,
            record,
        }
        .into();

        conn.close().await?;

        Ok(out)
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
        game_id: String,
    ) -> async_graphql::Result<Map> {
        let db = ctx.data_unchecked::<MySqlPool>();
        let mut conn = db.acquire().await?;

        let out = records_lib::map::get_map_from_uid(&mut conn, &game_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
            .map(Into::into);

        conn.close().await?;

        out
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
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();

        let date_sort_by = SortState::sql_order_by(&date_sort_by);

        let query = format!(
            "SELECT * FROM global_records r
            ORDER BY record_date {date_sort_by}
            LIMIT 100"
        );

        let mut records = sqlx::query_as::<_, models::Record>(&query).fetch(&db.mysql_pool);
        let mut ranked_records = Vec::with_capacity(records.size_hint().0);

        let mut conn = db.acquire().await?;

        while let Some(record) = records.next().await {
            let record = record?;

            let rank = get_rank(
                &mut conn,
                record.map_id,
                record.record_player_id,
                record.time,
                Default::default(),
            )
            .await?;

            ranked_records.push(models::RankedRecord { rank, record }.into());
        }

        conn.close().await?;

        Ok(ranked_records)
    }
}

struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn update_resources_content(
        &self,
        ctx: &async_graphql::Context<'_>,
        text: String,
    ) -> async_graphql::Result<models::ResourcesContent> {
        let db = ctx.data_unchecked::<Database>();

        let web_token = ctx
            .data_opt::<WebToken>()
            .ok_or_else(|| async_graphql::Error::new("Unauthorized"))?;
        auth::website_check_auth_for(db, &web_token.login, &web_token.token, privilege::ADMIN)
            .await?;

        let mut mysql_conn = db.mysql_pool.acquire().await?;

        sqlx::query("INSERT INTO resources_content (content, created_at) VALUES (?, SYSDATE())")
            .bind(text)
            .execute(&mut *mysql_conn)
            .await?;
        let res = sqlx::query_as("SELECT * FROM resources_content")
            .fetch_one(&mut *mysql_conn)
            .await?;

        mysql_conn.close().await?;

        Ok(res)
    }

    async fn calc_mappack_scores(
        &self,
        ctx: &async_graphql::Context<'_>,
        mappack_id: String,
    ) -> async_graphql::Result<Mappack> {
        self::mappack::get_mappack(ctx, mappack_id)
            .await
            .map_err(Into::into)
    }
}

type Schema = async_graphql::Schema<QueryRoot, MutationRoot, async_graphql::EmptySubscription>;

#[allow(clippy::let_and_return)]
fn create_schema(db: Database, client: Client) -> Schema {
    let schema =
        async_graphql::Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription)
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
            .data(client)
            .limit_depth(16)
            .finish();

    #[cfg(feature = "gql_schema")]
    {
        use std::fs::{self, OpenOptions};
        use std::io::Write;
        use std::path::Path;

        let schema_name = format!("schema_{}.graphql", chrono::Utc::now().timestamp());

        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(Path::new("schemas").join(&schema_name))
            .unwrap()
            .write_all(schema.sdl().as_bytes())
            .unwrap();

        fs::write("./etc/api_last_gql_schema", schema_name).unwrap();
    }

    schema
}

async fn index_graphql(
    request_id: RequestId,
    session: Session,
    schema: Data<Schema>,
    GraphQLRequest(request): GraphQLRequest,
) -> impl Responder {
    let web_token = session
        .get::<WebToken>(WEB_TOKEN_SESS_KEY)
        .expect("unable to retrieve web token");

    let request = {
        if let Some(web_token) = web_token {
            request.data(web_token)
        } else {
            request
        }
    };

    let mut result = schema.execute(request).await;
    for error in &mut result.errors {
        let ex = error
            .extensions
            .get_or_insert_with(ErrorExtensionValues::default);

        ex.set("request_id", Value::String(request_id.to_string()));
    }

    web::Json(result)
}

async fn index_playground() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(playground_source(GraphQLPlaygroundConfig::new(
            &crate::env().gql_endpoint,
        )))
}

pub fn graphql_route(db: Database, client: Client) -> Resource {
    web::resource("/graphql")
        .app_data(Data::new(create_schema(db, client)))
        .route(web::get().to(index_playground))
        .route(web::post().to(index_graphql))
}
