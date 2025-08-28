//! See [`records_lib::must`] module documentation.

use actix_web::{FromRequest, HttpMessage, HttpRequest};
use records_lib::RedisPool;
use sea_orm::DbConn;
use tracing_actix_web::RequestId;

use crate::{Res, utils::ExtractDbConn};

pub fn have_request_id(req: &HttpRequest) -> RequestId {
    *req.extensions()
        .get::<RequestId>()
        .expect("RequestId should be present")
}

pub fn have_redis_pool(req: &HttpRequest) -> RedisPool {
    <Res<records_lib::Database> as FromRequest>::extract(req)
        .into_inner()
        .unwrap()
        .0
        .redis_pool
}

pub fn have_dbconn(req: &HttpRequest) -> DbConn {
    <ExtractDbConn as FromRequest>::extract(req)
        .into_inner()
        .unwrap()
        .0
}
