use deadpool::managed::PoolError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecordsError {
    #[error(transparent)]
    MySql(#[from] sqlx::Error),
    #[error("unknown data store error")]
    Unknown,
    #[error("banned player")]
    BannedPlayer,
}

impl warp::reject::Reject for RecordsError {}

impl actix_web::ResponseError for RecordsError {
    fn error_response(&self) -> actix_web::HttpResponse {
        actix_web::HttpResponse::InternalServerError().finish()
    }
}

impl<E> From<PoolError<E>> for RecordsError {
    fn from(_: PoolError<E>) -> Self {
        Self::Unknown
    }
}