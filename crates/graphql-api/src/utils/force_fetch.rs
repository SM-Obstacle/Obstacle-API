//! What keeps the `forceFetchMxId` mutation from being used for a whole list of maps at once.

use std::sync::atomic::{AtomicBool, Ordering};

use async_graphql::{
    Request, ServerResult,
    extensions::{Extension, ExtensionContext, ExtensionFactory, NextPrepareRequest},
};

/// The single force fetch a GraphQL request is allowed.
///
/// Each request gets its own, and the first field to spend it is the only one which talks to MX.
/// Nothing stops a client from listing a hundred aliased `forceFetchMxId` in the same mutation, so
/// this is counted while the request runs rather than on the document: it holds wherever the field
/// ends up, aliases and fragments included.
#[derive(Default)]
pub struct ForceFetchBudget(AtomicBool);

impl ForceFetchBudget {
    /// Spends the budget, and returns whether it was still there.
    pub fn spend(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }
}

/// Gives a [`ForceFetchBudget`] to every incoming query.
pub struct ForceFetchBudgetExtension;

impl ExtensionFactory for ForceFetchBudgetExtension {
    fn create(&self) -> std::sync::Arc<dyn Extension> {
        std::sync::Arc::new(Self)
    }
}

#[async_graphql::async_trait::async_trait]
impl Extension for ForceFetchBudgetExtension {
    async fn prepare_request(
        &self,
        ctx: &ExtensionContext<'_>,
        request: Request,
        next: NextPrepareRequest<'_>,
    ) -> ServerResult<Request> {
        next.run(ctx, request.data(ForceFetchBudget::default()))
            .await
    }
}

#[cfg(test)]
mod tests {
    use async_graphql::{EmptyMutation, EmptySubscription, Schema, value};

    use super::{ForceFetchBudget, ForceFetchBudgetExtension};

    struct Query;

    #[async_graphql::Object]
    impl Query {
        /// Stands for any field which costs a request to MX.
        async fn spend(&self, ctx: &async_graphql::Context<'_>) -> bool {
            ctx.data_unchecked::<ForceFetchBudget>().spend()
        }
    }

    #[tokio::test]
    async fn each_query_gets_one_budget_and_only_one() {
        let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
            .extension(ForceFetchBudgetExtension)
            .finish();

        // Whatever the shape of the query, only the first field to ask gets it.
        let res = schema.execute("{ a: spend b: spend c: spend }").await;
        assert_eq!(
            res.data,
            value!({ "a": true, "b": false, "c": false }),
            "{:?}",
            res.errors
        );

        // And the next query starts with a fresh one.
        let res = schema.execute("{ a: spend }").await;
        assert_eq!(res.data, value!({ "a": true }), "{:?}", res.errors);
    }
}
