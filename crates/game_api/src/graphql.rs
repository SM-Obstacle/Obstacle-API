use actix_session::Session;
use actix_web::{HttpRequest, web};
use actix_web::{HttpResponse, Resource, Responder};
use async_graphql::ErrorExtensionValues;
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql_actix_web::GraphQLRequest;
use graphql_api::error::{ApiGqlError, ApiGqlErrorKind};
use graphql_api::schema::{Schema, create_schema};
use mkenv::prelude::*;
use records_lib::Database;
use reqwest::Client;
use tracing_actix_web::RequestId;

use crate::auth::{WEB_TOKEN_SESS_KEY, WebToken};
use crate::{ApiErrorKind, RecordsResult, Res, configure, internal};

async fn index_graphql(
    request_id: RequestId,
    client: Res<reqwest::Client>,
    req: HttpRequest,
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

            let mapped_err_type = if (100..200).contains(&err_type) || status_code.is_server_error()
            {
                error.message = "Internal server error".to_owned();
                configure::send_internal_err_msg_detached(
                    client.0.clone(),
                    req.head().clone(),
                    request_id,
                    err,
                );

                105 // Unknown type
            } else {
                err_type
            };

            extensions.set("error_code", mapped_err_type);
        }

        extensions.set("request_id", request_id.to_string());
    }

    Ok(web::Json(result))
}

async fn index_playground() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(playground_source(GraphQLPlaygroundConfig::new(
            &crate::env().gql_endpoint.get(),
        )))
}

pub fn graphql_route(db: Database, client: Client) -> Resource {
    web::resource("/graphql")
        .app_data(create_schema(db, client))
        .route(web::get().to(index_playground))
        .route(web::post().to(index_graphql))
}
