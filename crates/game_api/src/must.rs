//! See [`records_lib::must`] module documentation.

use actix_web::{FromRequest, HttpMessage, HttpRequest};
use sea_orm::DbConn;
use tracing_actix_web::RequestId;

use crate::utils::ExtractDbConn;

#[cfg(auth)]
use crate::Res;
#[cfg(auth)]
use records_lib::Database;

pub fn have_request_id(req: &HttpRequest) -> RequestId {
    *req.extensions()
        .get::<RequestId>()
        .expect("RequestId should be present")
}

#[cfg(auth)]
pub fn have_db(req: &HttpRequest) -> Res<Database> {
    req.app_data::<Database>()
        .expect("Database app data should be present")
        .clone()
        .into()
}

pub fn have_dbconn(req: &HttpRequest) -> DbConn {
    <ExtractDbConn as FromRequest>::extract(req)
        .into_inner()
        .expect("ExtractDbConnection::from_request must not return an error")
        .0
}
