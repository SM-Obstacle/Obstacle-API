use std::{
    convert::Infallible,
    future::{Ready, ready},
    ops::{Deref, DerefMut},
};

use actix_web::{
    FromRequest, HttpRequest, HttpResponse, Responder, body::MessageBody, dev::Payload,
};
use records_lib::{Database, models};
use serde::Serialize;

use crate::{RecordsResult, RecordsResultExt};

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

#[derive(Serialize, sqlx::FromRow)]
pub struct ApiStatus {
    pub at: chrono::NaiveDateTime,
    #[sqlx(flatten)]
    pub kind: models::ApiStatusKind,
}

pub async fn get_api_status(db: &Database) -> RecordsResult<ApiStatus> {
    let result = sqlx::query_as(
        "SELECT a.*, sh1.status_history_date as `at`
        FROM api_status_history sh1
        INNER JOIN api_status a ON a.status_id = sh1.status_id
        ORDER BY sh1.status_history_id DESC
        LIMIT 1",
    )
    .fetch_one(&db.mysql_pool)
    .await
    .with_api_err()?;

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
    type Error = Infallible;

    type Future = Ready<Result<Self, Infallible>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let client = req
            .app_data::<T>()
            .unwrap_or_else(|| panic!("{} should be present", std::any::type_name::<T>()))
            .clone();
        ready(Ok(Self(client)))
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
