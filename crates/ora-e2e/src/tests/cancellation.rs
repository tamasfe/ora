//! Tests for job cancellations.

use ora_client::{executor::IntoExecutionHandler, job_type::JobTypeExt, AdminClient};
use ora_server::{ServerOptions, Storage};
use tokio::time::sleep;

use crate::{jobs, util::log_audit_events};

/// A simple test that cancels a job.
pub async fn test_cancel_job<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora_client::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::wait_forever_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client.add_job(jobs::WaitForever.job()).await?;

    sleep(std::time::Duration::from_millis(300)).await;

    assert!(job.status().await?.is_running());

    job.cancel().await?;

    assert!(job.status().await?.is_failed());
    assert!(job.details().await?.cancelled);

    server.shutdown().await?;

    Ok(())
}
