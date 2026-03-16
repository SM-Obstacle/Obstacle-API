use std::sync::Arc;

use actix_http::RequestHead;
use actix_web::{HttpRequest, Scope, guard, web};
use actix_web::{HttpResponse, Responder};
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql::{ErrorExtensionValues, Executor};
use async_graphql_actix_web::{GraphQLRequest, GraphQLSubscription};
use futures::StreamExt;
use futures::stream::BoxStream;
use graphql_api::error::{ApiGqlError, ApiGqlErrorKind};
use graphql_api::schema::{Schema, create_schema};
use records_lib::Database;
use records_lib::records_notifier::LatestRecordsSubscription;
use reqwest::Client;
use tracing_actix_web::RequestId;

use crate::{ApiErrorKind, RecordsResult, Res, configure};

#[derive(Clone)]
struct ExecutorInventory {
    req_head: RequestHead,
    request_id: RequestId,
    client: reqwest::Client,
}

impl ExecutorInventory {
    fn mask_internal_errors(
        &self,
        mut response: async_graphql::Response,
    ) -> async_graphql::Response {
        for error in &mut response.errors {
            tracing::error!("Error encountered when processing GraphQL request: {error:?}");

            let api_error = error.source::<ApiGqlError>().cloned();

            let extensions = error
                .extensions
                .get_or_insert_with(ErrorExtensionValues::default);

            // Don't expose internal server errors
            if let Some(err) = api_error
                && let ApiGqlErrorKind::Lib(records_err) = err.kind()
            {
                let err = ApiErrorKind::Lib(records_err);
                let (err_type, status_code) = err.get_err_type_and_status_code();

                let mapped_err_type =
                    if (100..200).contains(&err_type) || status_code.is_server_error() {
                        error.message = "Internal server error".to_owned();
                        configure::send_internal_err_msg_detached(
                            self.client.clone(),
                            self.req_head.clone(),
                            self.request_id,
                            err,
                        );

                        105 // Unknown type
                    } else {
                        err_type
                    };

                extensions.set("error_code", mapped_err_type);
            }

            extensions.set("request_id", self.request_id.to_string());
        }

        response
    }
}

#[derive(Clone)]
struct GraphqlApiExecutor {
    schema: Schema,
    inventory: ExecutorInventory,
}

impl Executor for GraphqlApiExecutor {
    async fn execute(&self, request: async_graphql::Request) -> async_graphql::Response {
        let result = Executor::execute(&self.schema, request).await;
        self.inventory.mask_internal_errors(result)
    }

    fn execute_stream(
        &self,
        request: async_graphql::Request,
        session_data: Option<Arc<async_graphql::Data>>,
    ) -> BoxStream<'static, async_graphql::Response> {
        let inventory = self.inventory.clone();
        Executor::execute_stream(&self.schema, request, session_data)
            .map(move |response| inventory.mask_internal_errors(response))
            .boxed()
    }
}

async fn index_graphql(
    request_id: RequestId,
    client: Res<reqwest::Client>,
    req: HttpRequest,
    schema: Res<Schema>,
    GraphQLRequest(request): GraphQLRequest,
) -> RecordsResult<impl Responder> {
    let executor = GraphqlApiExecutor {
        schema: schema.0,
        inventory: ExecutorInventory {
            req_head: req.head().clone(),
            request_id,
            client: client.0,
        },
    };

    let result = executor.execute(request).await;

    Ok(web::Json(result))
}

async fn index_playground() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(playground_source(
            GraphQLPlaygroundConfig::new("/graphql")
                .subscription_endpoint("/graphql/subscriptions"),
        ))
}

async fn index_subscriptions(
    request_id: RequestId,
    client: Res<reqwest::Client>,
    schema: Res<Schema>,
    req: HttpRequest,
    payload: web::Payload,
) -> Result<impl Responder, actix_web::Error> {
    GraphQLSubscription::new(GraphqlApiExecutor {
        schema: schema.0,
        inventory: ExecutorInventory {
            req_head: req.head().clone(),
            request_id,
            client: client.0,
        },
    })
    .start(&req, payload)
}

pub fn graphql_route(
    db: Database,
    client: Client,
    records_sub: LatestRecordsSubscription,
) -> Scope {
    web::scope("/graphql")
        .app_data(create_schema(db, client, records_sub))
        .route("", web::get().to(index_playground))
        .route("", web::post().to(index_graphql))
        .route(
            "/subscriptions",
            web::get()
                .guard(guard::Header("upgrade", "websocket"))
                .to(index_subscriptions),
        )
}
