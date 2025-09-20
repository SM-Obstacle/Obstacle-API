use actix_session::Session;
use actix_web::web;
use actix_web::{HttpResponse, Resource, Responder};
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql::{ErrorExtensionValues, Value};
use async_graphql_actix_web::GraphQLRequest;
use graphql_api::schema::{Schema, create_schema};
use records_lib::Database;
use reqwest::Client;
use tracing_actix_web::RequestId;

use crate::auth::{WEB_TOKEN_SESS_KEY, WebToken};
use crate::{RecordsErrorKind, RecordsResult, Res, internal};

async fn index_graphql(
    request_id: RequestId,
    session: Session,
    schema: Res<Schema>,
    GraphQLRequest(request): GraphQLRequest,
) -> RecordsResult<impl Responder> {
    let web_token = session
        .get::<WebToken>(WEB_TOKEN_SESS_KEY)
        .map_err(|e| internal!("unable to retrieve web token: {e}"))?;

    let request = {
        if let Some(web_token) = web_token {
            request.data(web_token)
        } else {
            request
        }
    };

    let mut result = schema.execute(request).await;

    for error in &mut result.errors {
        tracing::error!("Error encountered when processing GraphQL request: {error:?}");

        // Don't expose internal server errors
        if let Some(err) = error.source::<RecordsErrorKind>() {
            let (err_type, status_code) = err.get_err_type_and_status_code();
            if (100..200).contains(&err_type) || status_code.is_server_error() {
                error.message = "Internal server error".to_owned();
            }
        }

        let ex = error
            .extensions
            .get_or_insert_with(ErrorExtensionValues::default);

        ex.set("request_id", Value::String(request_id.to_string()));
    }

    Ok(web::Json(result))
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
        .app_data(create_schema(db, client))
        .route(web::get().to(index_playground))
        .route(web::post().to(index_graphql))
}
