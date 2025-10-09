use std::{
    future::{Ready, ready},
    ops::{Deref, DerefMut},
};

use actix_web::{
    FromRequest, HttpRequest, HttpResponse, Responder, body::MessageBody, dev::Payload,
};
use entity::{api_status, api_status_history, types};
use records_lib::Database;
use sea_orm::{
    ConnectionTrait, DbConn, EntityTrait, FromQueryResult, QueryOrder, QuerySelect, prelude::Expr,
    sea_query::Asterisk,
};
use serde::Serialize;

use crate::{ApiErrorKind, RecordsResult, RecordsResultExt, internal};

/// Converts the provided body to a `200 OK` JSON responses.
pub fn json<T: Serialize, E>(obj: T) -> Result<HttpResponse, E> {
    Ok(HttpResponse::Ok().json(obj))
}

/// Checks for any repeated item in a slice.
pub fn any_repeated<T: PartialEq>(slice: &[T]) -> bool {
    for (i, t) in slice.iter().enumerate() {
        if slice.split_at(i + 1).1.iter().any(|x| x == t) {
            return true;
        }
    }
    false
}

#[derive(Serialize, FromQueryResult)]
pub struct ApiStatus {
    pub at: chrono::NaiveDateTime,
    #[sea_orm(nested)]
    pub kind: types::ApiStatusKind,
}

pub async fn get_api_status<C: ConnectionTrait>(conn: &C) -> RecordsResult<ApiStatus> {
    let result = api_status_history::Entity::find()
        .inner_join(api_status::Entity)
        .order_by_desc(api_status_history::Column::StatusHistoryId)
        .limit(1)
        .select_only()
        .column_as(api_status_history::Column::StatusHistoryDate, "at")
        .expr(Expr::col((api_status::Entity, Asterisk)))
        .into_model()
        .one(conn)
        .await
        .with_api_err()?
        .ok_or_else(|| {
            internal!("api_status and api_status_history must have at least one row in database")
        })?;

    Ok(result)
}

/// A resource handler, like [`Data`][d].
///
/// The difference with [`Data`][d] is that it doesn't use an [`Arc`](std::sync::Arc)
/// internally, but the [`Clone`] implementation of the inner type to implement [`FromRequest`].
///
/// [d]: actix_web::web::Data
#[derive(Clone)]
pub struct Res<T>(pub T);

impl<T> From<T> for Res<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> AsRef<T> for Res<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> AsMut<T> for Res<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut *self
    }
}

impl<T> Deref for Res<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Res<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Clone + 'static> FromRequest for Res<T> {
    type Error = ApiErrorKind;

    type Future = Ready<RecordsResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let obj = req
            .app_data::<T>()
            .ok_or_else(|| internal!("{} should be present", std::any::type_name::<T>()))
            .cloned();
        ready(obj.map(Self))
    }
}

pub struct ExtractDbConn(pub DbConn);

impl AsRef<DbConn> for ExtractDbConn {
    #[inline(always)]
    fn as_ref(&self) -> &DbConn {
        self
    }
}

impl AsMut<DbConn> for ExtractDbConn {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut DbConn {
        self
    }
}

impl Deref for ExtractDbConn {
    type Target = DbConn;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ExtractDbConn {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromRequest for ExtractDbConn {
    type Error = ApiErrorKind;

    type Future = Ready<RecordsResult<Self>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        ready(
            <Res<Database> as FromRequest>::from_request(req, payload)
                .into_inner()
                .map(|db| Self(db.0.sql_conn)),
        )
    }
}

pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R, B> Responder for Either<L, R>
where
    B: MessageBody + 'static,
    L: Responder<Body = B>,
    R: Responder<Body = B>,
{
    type Body = B;

    fn respond_to(self, req: &actix_web::HttpRequest) -> actix_web::HttpResponse<Self::Body> {
        match self {
            Either::Left(l) => <L as Responder>::respond_to(l, req),
            Either::Right(r) => <R as Responder>::respond_to(r, req),
        }
    }
}
