//! Interface for querying executor information.

use std::time::SystemTime;

use eyre::{Context, OptionExt};
use tonic::Request;

use crate::{AdminClient, JobTypeId, executor::ExecutorId, proto::admin::v1::ListExecutorsRequest};

/// Information about a registered executor.
pub struct ExecutorInfo {
    /// The unique identifier of the executor.
    pub id: ExecutorId,
    /// The name of the executor.
    pub name: Option<String>,
    /// The time the executor was last seen.
    pub last_seen_at: SystemTime,
    /// Queues belonging to the executor.
    pub queues: Vec<ExecutorQueueInfo>,
}

/// A job queue belonging to an executor.
pub struct ExecutorQueueInfo {
    /// The job type ID of the queue.
    pub job_type_id: JobTypeId,
    /// The number of active executions.
    pub active_executions: u64,
    /// The maximum number of concurrent executions allowed.
    pub max_concurrent_executions: u64,
}

impl AdminClient {
    /// List connected executors.
    pub async fn list_executors(&self) -> crate::Result<Vec<ExecutorInfo>> {
        self.inner
            .list_executors(Request::new(ListExecutorsRequest {}))
            .await?
            .into_inner()
            .executors
            .into_iter()
            .map(|j| {
                Ok(ExecutorInfo {
                    id: ExecutorId(j.id.parse().wrap_err("invalid executor ID")?),
                    name: j.name,
                    last_seen_at: j
                        .last_seen_at
                        .map(TryInto::try_into)
                        .transpose()
                        .wrap_err("invalid last_seen_at")?
                        .ok_or_eyre("missing last_seen_at")?,
                    queues: j
                        .queues
                        .into_iter()
                        .map(|q| {
                            Ok(ExecutorQueueInfo {
                                job_type_id: JobTypeId::new(
                                    q.job_type.ok_or_eyre("missing job type")?.id,
                                )?,
                                active_executions: q.active_executions,
                                max_concurrent_executions: q.max_concurrent_executions,
                            })
                        })
                        .collect::<Result<Vec<_>, crate::Error>>()?,
                })
            })
            .collect::<Result<Vec<_>, crate::Error>>()
    }
}
