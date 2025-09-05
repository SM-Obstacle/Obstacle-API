//! See [`records_lib::must`] module documentation.

use actix_web::{FromRequest, HttpRequest};
use records_lib::RedisPool;
use sea_orm::DbConn;

use crate::{Res, utils::ExtractDbConn};

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
