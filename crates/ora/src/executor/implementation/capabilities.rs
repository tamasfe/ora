use std::cmp;

use flume::Sender;

use crate::{
    executor::implementation::ExecutorJobQueue,
    proto::{
        executors::v1::{self, ExecutorCapabilities, executor_message::ExecutorMessageKind},
        jobs::v1::JobType,
    },
};

#[tracing::instrument(skip_all)]
pub(super) async fn send_capabilities(
    executor_name: &str,
    queues: &[ExecutorJobQueue],
    server: &Sender<ExecutorMessageKind>,
) -> crate::Result<()> {
    _ = server
        .send_async(ExecutorMessageKind::Capabilities(ExecutorCapabilities {
            name: executor_name.to_string(),
            job_queues: queues
                .iter()
                .map(|q| {
                    Ok(v1::ExecutorJobQueue {
                        job_type: Some(JobType {
                            id: q.job_type_id.to_string(),
                            description: q.description.clone(),
                            input_schema_json: Some(serde_json::to_string(&q.input_schema)?),
                            output_schema_json: Some(serde_json::to_string(&q.output_schema)?),
                        }),
                        max_concurrent_executions: cmp::max(q.max_concurrent_jobs, 1),
                    })
                })
                .collect::<crate::Result<Vec<_>>>()?,
        }))
        .await;

    tracing::debug!("executor capabilities sent");

    Ok(())
}
