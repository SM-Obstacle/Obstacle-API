use async_graphql::{
    EmptyMutation, SchemaBuilder, dataloader::DataLoader, extensions::ApolloTracing,
};
use records_lib::{
    Database,
    records_notifier::{LatestRecordsSubscription, RecordsNotifier},
};

use crate::{
    loaders::{
        checkpoint_times::CheckpointTimesLoader, event::EventLoader,
        event_category::EventCategoryLoader, event_edition_map::EventEditionMapLoader,
        map::MapLoader, map_average_cps_times::MapAverageCpsTimesLoader,
        map_average_rating::MapAverageRatingLoader, map_mx_id::MapMxIdLoader,
        map_score::MapScoreLoader, player::PlayerLoader, player_role::PlayerRoleLoader,
        player_score::PlayerScoreLoader, rating_kind::RatingKindLoader, try_count::TryCountLoader,
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
        .data(DataLoader::new(
            PlayerScoreLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            MapScoreLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            PlayerRoleLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            CheckpointTimesLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            TryCountLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            MapAverageCpsTimesLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            MapAverageRatingLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            RatingKindLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(
            EventEditionMapLoader(db.clone().sql_conn),
            tokio::spawn,
        ))
        .data(DataLoader::new(MapMxIdLoader(client.clone()), tokio::spawn))
        .data(db_clone.sql_conn)
        .data(db_clone.redis_pool)
        .data(db)
        .data(client)
        .limit_depth(16)
        .finish()
}
