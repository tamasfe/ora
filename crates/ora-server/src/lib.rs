//! Ora server implementation.

use std::{
    future::ready,
    num::NonZeroUsize,
    time::{Duration, SystemTime},
};

use executor_registry::{ExecutorRegistry, ExecutorRegistryOptions};
use futures::{future::select, StreamExt};
use ora_proto::{
    server::v1::{
        admin_service_client::AdminServiceClient, admin_service_server::AdminService,
        executor_service_client::ExecutorServiceClient, executor_service_server::ExecutorService,
    },
    snapshot::v1::{
        snapshot_service_client::SnapshotServiceClient, snapshot_service_server::SnapshotService,
    },
};
use ora_storage::{JobQueryFilters, ScheduleQueryFilters};
use scheduling::{
    create_executions, create_schedule_jobs, mark_executions_ready, schedule_executions,
    timer::spawn_timer,
};
use storage::StorageWrapper;
use tokio::time::{sleep, timeout};
use tonic::transport::Channel;
use tracing::Instrument;
use wgroup::WaitGroup;

pub(crate) mod scheduling;
pub(crate) mod time;

pub(crate) mod admin;
pub(crate) mod events;
pub(crate) mod executor_registry;
pub(crate) mod snapshot;
pub(crate) mod storage;

#[cfg(feature = "metrics")]
pub(crate) mod metrics;

pub use ora_storage::{Storage, StorageSnapshot};

pub use events::{AuditEvent, AuditEventKind};

/// Re-exported types.
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;
/// Re-exported types.
pub type IndexSet<T> = indexmap::IndexSet<T, ahash::RandomState>;

pub use ora_timer::TimerOptions;

/// Options for the server.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Options for the timer.
    pub timer: ora_timer::TimerOptions,
    /// The timeout for an executor to be considered dead.
    pub executor_heartbeat_timeout: std::time::Duration,
    /// Buffer size for internal channels.
    pub timer_buffer_size: NonZeroUsize,
    /// Buffer size for internal events.
    pub event_buffer_size: NonZeroUsize,
    /// Bookkeeping interval for various tasks,
    /// the server is mostly event-driven but some
    /// tasks are run periodically.
    ///
    /// Even event-driven tasks are run periodically
    /// to ensure that they are not stuck.
    pub bookkeeping_interval: std::time::Duration,
    /// Executor shutdown timeout.
    pub executor_shutdown_timeout: std::time::Duration,
    /// Delete inactive jobs after this duration.
    ///
    /// By default, jobs are never deleted.
    pub max_job_age: Option<std::time::Duration>,
    /// Delete inactive schedules after this duration.
    ///
    /// By default, schedules are never deleted.
    pub max_schedule_age: Option<std::time::Duration>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            timer: Default::default(),
            executor_heartbeat_timeout: Duration::from_secs(60),
            timer_buffer_size: NonZeroUsize::new(100_000).unwrap(),
            event_buffer_size: NonZeroUsize::new(100_000).unwrap(),
            bookkeeping_interval: Duration::from_secs(5),
            executor_shutdown_timeout: Duration::from_secs(10),
            max_job_age: None,
            max_schedule_age: None,
        }
    }
}

/// A running server instance.
#[must_use = "the server needs to be explicitly stopped"]
pub struct Server<S>
where
    S: ora_storage::Storage,
{
    storage: S,
    executor_registry: executor_registry::ExecutorRegistry<StorageWrapper<S>>,
    admin: admin::Admin<StorageWrapper<S>>,
    options: ServerOptions,
    event_bus: events::EventBus,
    wg: Option<WaitGroup>,
}

impl<S> Server<S>
where
    S: ora_storage::Storage,
{
    /// Start a new server with the given storage backend and options.
    ///
    /// The server will spawn backgrounds tasks that will
    /// run until the server is stopped.
    pub fn spawn(storage: S, options: ServerOptions) -> eyre::Result<Self> {
        let wg = WaitGroup::new();
        let event_bus = events::EventBus::new(options.event_buffer_size.get());

        let storage = StorageWrapper::new(storage, event_bus.clone());

        let executor_registry = executor_registry::ExecutorRegistry::new(
            storage.clone(),
            ExecutorRegistryOptions {
                executor_timeout: options.executor_heartbeat_timeout,
            },
            wg.handle(),
            event_bus.clone(),
        );
        let admin = admin::Admin::new(
            storage.clone(),
            wg.handle(),
            event_bus.clone(),
            executor_registry.clone(),
        );

        let (mut pending_buf_producer, pending_buf_consumer) =
            rtrb::RingBuffer::new(options.timer_buffer_size.get());

        let (ready_buf_producer, mut ready_buf_consumer) =
            rtrb::RingBuffer::new(options.timer_buffer_size.get());

        spawn_timer(
            ready_buf_producer,
            pending_buf_consumer,
            wg.add_with("timer"),
            options.timer,
            event_bus.clone(),
        )?;

        tokio::spawn({
            let guard = wg.add_with("create_job_executions");
            let backend = storage.clone();
            let event_bus = event_bus.clone();

            async move {
                loop {
                    tracing::trace!("running create_job_executions");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = create_executions(&event_bus, &backend).await {
                        tracing::error!(?error, "failed to create job executions");
                    }

                    let mut events = event_bus
                        .subscribe_job_events()
                        .filter(|event| ready(matches!(event, events::JobEvent::JobsCreated)));
                    _ = timeout(options.bookkeeping_interval, events.next()).await;
                }
            }
            .instrument(tracing::info_span!("create_job_executions"))
        });

        tokio::spawn({
            let guard = wg.add_with("schedule_executions");
            let backend = storage.clone();
            let event_bus = event_bus.clone();

            async move {
                let mut last_scheduled_id = None;

                loop {
                    tracing::trace!("running schedule_executions");

                    if guard.is_waiting() {
                        break;
                    }

                    match schedule_executions(
                        &backend,
                        &mut pending_buf_producer,
                        last_scheduled_id,
                    )
                    .await
                    {
                        Ok(last_id) => {
                            last_scheduled_id = last_id.or(last_scheduled_id);
                        }
                        Err(error) => {
                            tracing::error!(?error, "failed to schedule executions");
                        }
                    }

                    let mut events = event_bus.subscribe_execution_events().filter(|event| {
                        ready(matches!(event, events::ExecutionEvent::ExecutionsAdded))
                    });
                    _ = timeout(options.bookkeeping_interval, events.next()).await;
                }
            }
            .instrument(tracing::info_span!("schedule_executions"))
        });

        tokio::spawn({
            let guard = wg.add_with("ready_executions");
            let backend = storage.clone();
            let event_bus = event_bus.clone();

            async move {
                loop {
                    tracing::trace!("running ready_executions");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) =
                        mark_executions_ready(&event_bus, &backend, &mut ready_buf_consumer).await
                    {
                        tracing::error!(?error, "failed to mark executions ready");
                    }

                    let mut events = event_bus.subscribe_execution_events().filter(|event| {
                        ready(matches!(
                            event,
                            events::ExecutionEvent::TimedExecutionsReady
                        ))
                    });
                    _ = timeout(options.bookkeeping_interval, events.next()).await;
                }
            }
            .instrument(tracing::info_span!("ready_executions"))
        });

        tokio::spawn({
            let guard = wg.add_with("assign_executions");
            let executor_registry = executor_registry.clone();
            let event_bus = event_bus.clone();

            async move {
                loop {
                    tracing::trace!("running assign_executions");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = executor_registry.assign_executions().await {
                        tracing::error!(?error, "failed to assign executions");
                    }

                    let mut execution_events =
                        event_bus.subscribe_execution_events().filter(|event| {
                            ready(matches!(
                                event,
                                events::ExecutionEvent::ExecutionsReadyToRun
                            ))
                        });
                    let mut executor_events =
                        event_bus.subscribe_executor_events().filter(|event| {
                            ready(matches!(event, events::ExecutorEvent::ExecutorReady))
                        });

                    _ = timeout(
                        options.bookkeeping_interval,
                        select(execution_events.next(), executor_events.next()),
                    )
                    .await;
                }
            }
            .instrument(tracing::info_span!("assign_executions"))
        });

        tokio::spawn({
            let guard = wg.add_with("execution_timeouts");
            let executor_registry = executor_registry.clone();
            let event_bus = event_bus.clone();

            async move {
                loop {
                    tracing::trace!("running execution_timeouts");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = executor_registry.fail_timed_out_executions().await {
                        tracing::error!(?error, "failed to fail timed out executions");
                    }

                    let mut events = event_bus.subscribe_execution_events().filter(|event| {
                        ready(matches!(
                            event,
                            events::ExecutionEvent::TimedExecutionsReady
                        ))
                    });
                    _ = timeout(options.bookkeeping_interval, events.next()).await;
                }
            }
            .instrument(tracing::info_span!("execution_timeouts"))
        });

        tokio::spawn({
            let guard = wg.add_with("reap_dead_executors");
            let executor_registry = executor_registry.clone();

            async move {
                loop {
                    tracing::trace!("running reap_dead_executors");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = executor_registry.reap_dead_executors().await {
                        tracing::error!(?error, "failed to reap dead executors");
                    }

                    sleep(options.bookkeeping_interval).await;
                }
            }
            .instrument(tracing::info_span!("reap_dead_executors"))
        });

        tokio::spawn({
            let guard = wg.add_with("clean_up_orphan_executions");
            let executor_registry = executor_registry.clone();

            async move {
                loop {
                    tracing::trace!("running clean_up_orphan_executions");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = executor_registry.clean_up_orphan_executions().await {
                        tracing::error!(?error, "failed to clean up orphan executions");
                    }

                    sleep(options.bookkeeping_interval).await;
                }
            }
            .instrument(tracing::info_span!("clean_up_orphan_executions"))
        });

        if let Some(max_job_age) = options.max_job_age {
            tokio::spawn({
                let guard = wg.add_with("remove_old_jobs");
                let storage = storage.clone();

                async move {
                    loop {
                        tracing::trace!("running remove_old_jobs");

                        if guard.is_waiting() {
                            break;
                        }

                        let Some(after) = SystemTime::now().checked_sub(max_job_age) else {
                            sleep(options.bookkeeping_interval).await;
                            continue;
                        };

                        if let Err(error) = storage
                            .delete_jobs(JobQueryFilters {
                                active: Some(false),
                                created_before: Some(after),
                                ..Default::default()
                            })
                            .await
                        {
                            tracing::error!(?error, "failed to clean up jobs");
                        }

                        sleep(options.bookkeeping_interval).await;
                    }
                }
                .instrument(tracing::info_span!("remove_old_jobs"))
            });
        }

        if let Some(max_schedule_age) = options.max_schedule_age {
            tokio::spawn({
                let guard = wg.add_with("remove_old_schedules");
                let storage = storage.clone();

                async move {
                    loop {
                        tracing::trace!("running remove_old_schedules");

                        if guard.is_waiting() {
                            break;
                        }

                        let Some(after) = SystemTime::now().checked_sub(max_schedule_age) else {
                            sleep(options.bookkeeping_interval).await;
                            continue;
                        };

                        if let Err(error) = storage
                            .delete_schedules(ScheduleQueryFilters {
                                active: Some(false),
                                created_before: Some(after),
                                ..Default::default()
                            })
                            .await
                        {
                            tracing::error!(?error, "failed to clean up schedules");
                        }

                        sleep(options.bookkeeping_interval).await;
                    }
                }
                .instrument(tracing::info_span!("remove_old_schedules"))
            });
        }

        tokio::spawn({
            let guard = wg.add_with("create_schedule_jobs");
            let event_bus = event_bus.clone();
            let storage = storage.clone();

            async move {
                loop {
                    tracing::trace!("running create_schedule_jobs");

                    if guard.is_waiting() {
                        break;
                    }

                    if let Err(error) = create_schedule_jobs(&event_bus, &storage).await {
                        tracing::error!(?error, "failed to create schedule jobs");
                    }

                    let mut execution_events =
                        event_bus.subscribe_execution_events().filter(|event| {
                            ready(matches!(event, events::ExecutionEvent::ExecutionsFinished))
                        });
                    let mut schedule_events =
                        event_bus.subscribe_schedule_events().filter(|event| {
                            ready(matches!(event, events::ScheduleEvent::SchedulesAdded))
                        });

                    _ = timeout(
                        options.bookkeeping_interval,
                        select(execution_events.next(), schedule_events.next()),
                    )
                    .await;
                }
            }
            .instrument(tracing::info_span!("create_schedule_jobs"))
        });

        #[cfg(feature = "metrics")]
        tokio::spawn({
            let storage = storage.clone();
            let event_bus = event_bus.clone();

            async move {
                if let Err(error) = crate::metrics::collect_metrics(&storage, &event_bus).await {
                    tracing::error!(?error, "error collecting metrics");
                }
            }
        });

        Ok(Self {
            storage: storage.inner.clone(),
            executor_registry,
            admin,
            options,
            event_bus,
            wg: Some(wg),
        })
    }

    /// Get a handle to the admin gRPC service.
    pub fn admin_service(&self) -> impl AdminService {
        self.admin.clone()
    }

    /// Get a handle to the executor service.
    pub fn executor_service(&self) -> impl ExecutorService {
        self.executor_registry.clone()
    }

    /// Get an admin service client that connects to the server
    /// using an in-memory transport.
    ///
    /// Make sure to reuse the client as every call to this function
    /// will spawn a new task that is only cleaned up
    /// once the server is dropped.
    #[allow(clippy::missing_panics_doc)]
    pub fn admin_service_client(&self) -> AdminServiceClient<Channel> {
        use hyper_util::rt::TokioIo;
        use ora_proto::server::v1::admin_service_server::AdminServiceServer;
        use tonic::transport::{Endpoint, Uri};

        let (client, server) = tokio::io::duplex(1024);

        let admin = self.admin_service();
        let wg = self.wg.as_ref().unwrap().handle().add();

        tokio::spawn(async move {
            let srv = tonic::transport::Server::builder()
                .add_service(AdminServiceServer::new(admin).max_decoding_message_size(usize::MAX))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)));

            let waiting = wg.waiting();

            tokio::select! {
                _ = waiting => {}
                serve_result = srv => {
                    if let Err(error) = serve_result {
                        tracing::error!(?error, "error during admin service serve");
                    }
                }
            }
        });

        let mut client = Some(client);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                let client = client.take();

                async move {
                    if let Some(client) = client {
                        Ok(TokioIo::new(client))
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Client already taken",
                        ))
                    }
                }
            }));

        AdminServiceClient::new(channel)
    }

    /// Get an executor service client that connects to the server
    /// using an in-memory transport.
    ///
    /// Make sure to reuse the client as every call to this function
    /// will spawn a new task that is only cleaned up
    /// once the server is dropped.
    #[allow(clippy::missing_panics_doc)]
    pub fn executor_service_client(&self) -> ExecutorServiceClient<Channel> {
        use hyper_util::rt::TokioIo;
        use ora_proto::server::v1::executor_service_server::ExecutorServiceServer;
        use tonic::transport::{Endpoint, Uri};

        let (client, server) = tokio::io::duplex(1024);

        let executor_registry = self.executor_service();
        let wg = self.wg.as_ref().unwrap().handle().add();

        tokio::spawn(async move {
            let srv = tonic::transport::Server::builder()
                .add_service(
                    ExecutorServiceServer::new(executor_registry)
                        .max_decoding_message_size(usize::MAX),
                )
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)));

            let waiting = wg.waiting();

            tokio::select! {
                _ = waiting => {}
                serve_result = srv => {
                    if let Err(error) = serve_result {
                        tracing::error!(?error, "error during admin service serve");
                    }
                }
            }
        });

        let mut client = Some(client);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                let client = client.take();

                async move {
                    if let Some(client) = client {
                        Ok(TokioIo::new(client))
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Client already taken",
                        ))
                    }
                }
            }));

        ExecutorServiceClient::new(channel)
    }

    /// Get the server options.
    pub fn options(&self) -> &ServerOptions {
        &self.options
    }

    /// Subscribe to system events.
    pub fn events(
        &self,
    ) -> impl futures::Stream<Item = AuditEvent> + Unpin + Send + Sync + 'static {
        self.event_bus.subscribe_audit_events()
    }

    /// Shutdown the server.
    #[tracing::instrument(skip_all)]
    pub async fn shutdown(mut self) -> eyre::Result<()> {
        Self::shutdown_impl(
            self.wg.take().unwrap(),
            self.executor_registry.clone(),
            self.options.clone(),
        )
        .await
    }

    async fn shutdown_impl(
        wg: WaitGroup,
        executor_registry: ExecutorRegistry<StorageWrapper<S>>,
        options: ServerOptions,
    ) -> eyre::Result<()> {
        tracing::info!("shutting down ora server");

        let mut running_components = wg.all_done_stream();

        tokio::spawn(async move {
            if let Err(error) = executor_registry
                .shutdown(Some(options.executor_shutdown_timeout))
                .await
            {
                tracing::error!(?error, "error during executor registry shutdown");
            }
        });

        while let Some((component_count, components)) = running_components.next().await {
            tracing::debug!(
                component_count = component_count,
                components = ?components,
                "waiting for components to shut down",
            );
        }

        Ok(())
    }
}

impl<S> Server<S>
where
    S: ora_storage::Storage + StorageSnapshot,
{
    /// Get a handle to the snapshot service.
    #[allow(clippy::missing_panics_doc)]
    pub fn snapshot_service(&self) -> impl SnapshotService {
        snapshot::SnapshotInterface::new(
            self.storage.clone(),
            self.wg.as_ref().unwrap().handle(),
            self.event_bus.clone(),
        )
    }

    /// Get a snapshot service client that connects to the server
    /// using an in-memory transport.
    ///
    /// Make sure to reuse the client as every call to this function
    /// will spawn a new task that is only cleaned up
    /// once the server is dropped.
    #[allow(clippy::missing_panics_doc)]
    pub fn snapshot_service_client(&self) -> SnapshotServiceClient<Channel> {
        use hyper_util::rt::TokioIo;
        use ora_proto::snapshot::v1::snapshot_service_server::SnapshotServiceServer;
        use tonic::transport::{Endpoint, Uri};

        let (client, server) = tokio::io::duplex(1024);

        let svc = self.snapshot_service();
        let wg = self.wg.as_ref().unwrap().handle().add();

        tokio::spawn(async move {
            let srv = tonic::transport::Server::builder()
                .add_service(SnapshotServiceServer::new(svc).max_decoding_message_size(usize::MAX))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)));

            let waiting = wg.waiting();

            tokio::select! {
                _ = waiting => {}
                serve_result = srv => {
                    if let Err(error) = serve_result {
                        tracing::error!(?error, "error during admin service serve");
                    }
                }
            }
        });

        let mut client = Some(client);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                let client = client.take();

                async move {
                    if let Some(client) = client {
                        Ok(TokioIo::new(client))
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Client already taken",
                        ))
                    }
                }
            }));

        SnapshotServiceClient::new(channel)
    }
}

impl<S> Drop for Server<S>
where
    S: ora_storage::Storage,
{
    fn drop(&mut self) {
        let Some(wg) = self.wg.take() else {
            return;
        };

        let stop_fut =
            Self::shutdown_impl(wg, self.executor_registry.clone(), self.options.clone());

        tracing::warn!("server instance dropped, attempting shutdown in the background");
        tokio::spawn(async move {
            if let Err(error) = stop_fut.await {
                tracing::error!(?error, "error during server shutdown");
            }
        });
    }
}
