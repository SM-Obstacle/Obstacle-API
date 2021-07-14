use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecordsError {
    #[error(transparent)]
    MySql(#[from] sqlx::Error),
    #[error("unknown data store error")]
    Unknown,
}

impl warp::reject::Reject for RecordsError {}
