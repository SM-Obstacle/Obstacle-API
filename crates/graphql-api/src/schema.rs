use async_graphql::{
    EmptyMutation, SchemaBuilder, dataloader::DataLoader, extensions::ApolloTracing,
};
use records_lib::{
    Database,
    records_notifier::{LatestRecordsSubscription, RecordsNotifier},
};

use crate::{
    loaders::{
        event::EventLoader, event_category::EventCategoryLoader, map::MapLoader,
        player::PlayerLoader,
    },
    objects::root::QueryRoot,
    subscriptions::root::SubscriptionRoot,
};

pub type Schema = async_graphql::Schema<QueryRoot, EmptyMutation, SubscriptionRoot>;

fn create_schema_impl(
    records_sub: LatestRecordsSubscription,
) -> SchemaBuilder<QueryRoot, EmptyMutation, SubscriptionRoot> {
    async_graphql::Schema::build(QueryRoot, EmptyMutation, SubscriptionRoot::new(records_sub))
}

pub fn create_schema_standalone() -> Schema {
    let dummy = RecordsNotifier::default();
    create_schema_impl(dummy.get_subscription()).finish()
}

pub fn create_schema(
    db: Database,
    client: reqwest::Client,
    records_sub: LatestRecordsSubscription,
) -> Schema {
    let db_clone = db.clone();

    create_schema_impl(records_sub)
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
