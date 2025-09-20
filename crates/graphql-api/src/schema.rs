use async_graphql::{
    EmptyMutation, EmptySubscription, SchemaBuilder, dataloader::DataLoader,
    extensions::ApolloTracing,
};
use records_lib::Database;

use crate::{
    loaders::{
        event::EventLoader, event_category::EventCategoryLoader, map::MapLoader,
        player::PlayerLoader,
    },
    objects::root::QueryRoot,
};

pub type Schema = async_graphql::Schema<
    QueryRoot,
    async_graphql::EmptyMutation,
    async_graphql::EmptySubscription,
>;

fn create_schema_impl() -> SchemaBuilder<QueryRoot, EmptyMutation, EmptySubscription> {
    async_graphql::Schema::build(
        QueryRoot,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
}

pub fn create_schema_standalone() -> Schema {
    create_schema_impl().finish()
}

pub fn create_schema(db: Database, client: reqwest::Client) -> Schema {
    let db_clone = db.clone();

    create_schema_impl()
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
        .finish()
}
