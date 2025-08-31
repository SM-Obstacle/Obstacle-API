use actix_session::Session;
use actix_web::web::{self, Data};
use actix_web::{HttpResponse, Resource, Responder};
use async_graphql::dataloader::DataLoader;
use async_graphql::extensions::ApolloTracing;
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql::{Enum, ErrorExtensionValues, Value};
use async_graphql_actix_web::GraphQLRequest;
use entity::{event as event_entity, event_edition, global_records, players, records};
use records_lib::opt_event::OptEvent;
use records_lib::ranks::get_rank;
use records_lib::transaction;
use records_lib::{Database, RedisConnection, must};
use reqwest::Client;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{ExprTrait, Func};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbConn, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    StreamTrait,
};
use std::vec::Vec;
use tracing_actix_web::RequestId;

use crate::auth::{WEB_TOKEN_SESS_KEY, WebToken};
use crate::graphql::map::MapLoader;
use crate::graphql::player::PlayerLoader;

use self::event::{Event, EventCategoryLoader, EventEdition, EventLoader};
use self::map::Map;
use self::mappack::Mappack;
use self::player::Player;
use self::record::RankedRecord;

mod event;
mod map;
mod mappack;
mod player;
mod rating;
mod record;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Enum)]
pub(crate) enum SortState {
    Sort,
    Reverse,
}

struct QueryRoot;

#[async_graphql::Object]
impl QueryRoot {
    async fn event_edition_from_mx_id(
        &self,
        ctx: &async_graphql::Context<'_>,
        mx_id: i64,
    ) -> async_graphql::Result<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_edition::Entity::find()
            .filter(event_edition::Column::MxId.eq(mx_id))
            .one(conn)
            .await?;

        Ok(match edition {
            Some(edition) => Some(EventEdition::from_inner(conn, edition).await?),
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

    async fn event(
        &self,
        ctx: &async_graphql::Context<'_>,
        handle: String,
    ) -> async_graphql::Result<Event> {
        let conn = ctx.data_unchecked::<DbConn>();
        let event = must::have_event_handle(conn, &handle).await?;

        Ok(event.into())
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Vec<Event>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let r = event_entity::Entity::find()
            .reverse_join(event_edition::Entity)
            .filter(
                Expr::col(event_edition::Column::StartDate)
                    .lt(Func::cust("SYSDATE"))
                    .and(
                        event_edition::Column::Ttl
                            .is_null()
                            .or(Func::cust("SYSDATE").lt(Func::cust("TIMESTAMPADD")
                                .arg(Expr::custom_keyword("SECOND"))
                                .arg(Expr::col(event_edition::Column::Ttl))
                                .arg(Expr::col(event_edition::Column::StartDate)))),
                    ),
            )
            .group_by(event_entity::Column::Id)
            .into_model()
            .all(conn)
            .await?;

        Ok(r)
    }

    async fn record(
        &self,
        ctx: &async_graphql::Context<'_>,
        record_id: u32,
    ) -> async_graphql::Result<RankedRecord> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_record(txn, &mut redis_conn, record_id, Default::default()).await
        })
        .await
    }

    async fn map(
        &self,
        ctx: &async_graphql::Context<'_>,
        game_id: String,
    ) -> async_graphql::Result<Map> {
        let conn = ctx.data_unchecked::<DbConn>();

        records_lib::map::get_map_from_uid(conn, &game_id)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Map not found."))
            .map(Into::into)
    }

    async fn player(
        &self,
        ctx: &async_graphql::Context<'_>,
        login: String,
    ) -> async_graphql::Result<Player> {
        let conn = ctx.data_unchecked::<DbConn>();

        let player = players::Entity::find()
            .filter(players::Column::Login.eq(login))
            .one(conn)
            .await?
            .ok_or_else(|| async_graphql::Error::new("Player not found."))?;

        Ok(player.into())
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> async_graphql::Result<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();
        let mut redis_conn = db.redis_pool.get().await?;

        transaction::within(conn, async |txn| {
            get_records(txn, &mut redis_conn, date_sort_by, Default::default()).await
        })
        .await
    }
}

async fn get_record<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    record_id: u32,
    event: OptEvent<'_>,
) -> async_graphql::Result<RankedRecord> {
    let record = records::Entity::find_by_id(record_id).one(conn).await?;

    let Some(record) = record else {
        return Err(async_graphql::Error::new("Record not found."));
    };

    let out = records::RankedRecord {
        rank: get_rank(
            conn,
            redis_conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?,
        record,
    }
    .into();

    Ok(out)
}

async fn get_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    date_sort_by: Option<SortState>,
    event: OptEvent<'_>,
) -> async_graphql::Result<Vec<RankedRecord>> {
    let records = global_records::Entity::find()
        .order_by(
            global_records::Column::RecordDate,
            match date_sort_by {
                Some(SortState::Reverse) => sea_orm::Order::Asc,
                _ => sea_orm::Order::Desc,
            },
        )
        .limit(100)
        .all(conn)
        .await?;

    let mut ranked_records = Vec::with_capacity(records.len());

    for record in records {
        let rank = get_rank(
            conn,
            redis_conn,
            record.map_id,
            record.record_player_id,
            record.time,
            event,
        )
        .await?;

        ranked_records.push(
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        );
    }

    Ok(ranked_records)
}

type Schema = async_graphql::Schema<
    QueryRoot,
    async_graphql::EmptyMutation,
    async_graphql::EmptySubscription,
>;

#[allow(clippy::let_and_return)]
fn create_schema(db: Database, client: Client) -> Schema {
    let db_clone = db.clone();

    let schema = async_graphql::Schema::build(
        QueryRoot,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .extension(ApolloTracing)
    .data(DataLoader::new(
        PlayerLoader(db.clone().sql_conn),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        MapLoader(db.clone().sql_conn),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        EventLoader(db.clone().sql_conn),
        tokio::spawn,
    ))
    .data(DataLoader::new(
        EventCategoryLoader(db.clone().sql_conn),
        tokio::spawn,
    ))
    .data(db_clone.sql_conn)
    .data(db_clone.redis_pool)
    .data(db)
    .data(client)
    .limit_depth(16)
    .finish();

    #[cfg(feature = "gql_schema")]
    {
        use std::fs::OpenOptions;
        use std::io::Write;

        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open("schema.graphql")
            .unwrap()
            .write_all(schema.sdl().as_bytes())
            .unwrap();

        tracing::info!("Generated GraphQL schema file to schema.graphql");
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
