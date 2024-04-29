//! See [`records_lib::must`] module documentation.

use actix_web::{HttpMessage, HttpRequest};
use records_lib::Database;
use tracing_actix_web::RequestId;

use crate::Res;

pub fn have_request_id(req: &HttpRequest) -> RequestId {
    *req.extensions()
        .get::<RequestId>()
        .expect("RequestId should be present")
}

pub fn have_db(req: &HttpRequest) -> Res<Database> {
    req.app_data::<Database>()
        .expect("Database app data should be present")
        .clone()
        .into()
}
