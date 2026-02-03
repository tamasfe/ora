//! Error types used in this crate.

use std::convert::Infallible;

use crate::job_type::InvalidJobTypeId;

/// The result type used in this library.
pub type Result<T> = std::result::Result<T, Error>;

/// A common error type used in this library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid job type ID.
    #[error("{0}")]
    InvalidJobTypeId(#[from] InvalidJobTypeId),
    /// A JSON error.
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    /// A gRPC error.
    #[error("{0}")]
    Grpc(#[from] tonic::Status),
    /// A generic error, usually as a result of a bug.
    #[error("{0}")]
    Generic(#[from] eyre::Report),
    /// A given job has failed with the given reason.
    #[error("job failed: {0}")]
    JobFailed(String),
    /// A given job was cancelled.
    #[error("job cancelled")]
    JobCancelled,
    /// The cron expression is not valid.
    #[error("invalid cron expression: {0}")]
    InvalidCronExpression(cronexpr::Error),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}
