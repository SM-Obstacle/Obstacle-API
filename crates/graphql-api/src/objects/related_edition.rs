use crate::objects::{event_edition::EventEdition, map::Map};

#[derive(async_graphql::SimpleObject)]
pub struct RelatedEdition<'a> {
    pub map: Map,
    /// Tells the website to redirect to the event map page instead of the regular map page.
    ///
    /// This avoids to have access to the `/map/X_benchmark` page for example, because a Benchmark
    /// map won't have any record in this context. Thus, it should be redirected to
    /// `/event/benchmark/2/map/X_benchmark`.
    pub redirect_to_event: bool,
    pub edition: EventEdition<'a>,
}
