use std::sync::{atomic::Ordering, Arc};

use eyre::Context;
use ora_common::{schedule::ScheduleDefinition, task::WorkerSelector, timeout::TimeoutPolicy};
use ora_scheduler::store::{
    schedule::{ActiveSchedule, SchedulerScheduleStoreEvent},
    task::{PendingTask, SchedulerTaskStoreEvent},
};
use ora_worker::store::{ReadyTask, WorkerStoreEvent};
use sqlx::{
    postgres::{PgListener, PgNotification, PgRow},
    query,
    types::Json,
    Executor, PgPool, Postgres, Row,
};
use time::OffsetDateTime;
use tokio::{sync::broadcast, time::MissedTickBehavior};
use uuid::Uuid;

use super::task_definition_from_row;
use crate::{models::DbEvent, DbStore};

pub(crate) mod event {
    pub(crate) const TASK_ADDED: &str = "ora_task_added";
    pub(crate) const TASK_CANCELLED: &str = "ora_task_cancelled";
    pub(crate) const TASK_READY: &str = "ora_task_ready";
    // This is here for the sake of completeness, it's not used anywhere.
    #[allow(dead_code)]
    pub(crate) const TASK_STARTED: &str = "ora_task_started";
    pub(crate) const TASK_SUCCEEDED: &str = "ora_task_succeeded";
    pub(crate) const TASK_FAILED: &str = "ora_task_failed";

    pub(crate) const SCHEDULE_ADDED: &str = "ora_schedule_added";
    pub(crate) const SCHEDULE_CANCELLED: &str = "ora_schedule_cancelled";
}

impl DbStore<Postgres> {
    pub(super) async fn setup_poll_loop(&self) -> eyre::Result<()> {
        let opts = self.options.clone();
        let sender = self.events.clone();
        let store_count = Arc::downgrade(&self.store_count);

        let mut interval = tokio::time::interval(opts.poll_interval.try_into().unwrap());
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let db = self.db.clone();

        let mut last_full_check = OffsetDateTime::now_utc();

        let mut listener = PgListener::connect_with(&db).await?;
        listener
            .listen_all([
                event::TASK_ADDED,
                event::TASK_CANCELLED,
                event::TASK_READY,
                event::TASK_SUCCEEDED,
                event::TASK_FAILED,
                event::SCHEDULE_ADDED,
                event::SCHEDULE_CANCELLED,
            ])
            .await?;

        let global_ws = self.worker_selectors.clone();
        let global_ws_ver = self.worker_selector_version.clone();

        let mut worker_selector_version = global_ws_ver.load(Ordering::Relaxed);
        let mut query_worker_selectors = global_ws
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .map(Json)
            .collect::<Vec<_>>();

        let mut update_worker_selectors =
            move |query_worker_selectors: &mut Vec<Json<WorkerSelector>>| {
                let global_ws_version = global_ws_ver.load(Ordering::Relaxed);

                if worker_selector_version == global_ws_version {
                    return;
                }

                worker_selector_version = global_ws_version;

                *query_worker_selectors = global_ws
                    .lock()
                    .unwrap()
                    .iter()
                    .cloned()
                    .map(Json)
                    .collect::<Vec<_>>();
            };

        tokio::spawn(async move {
            loop {
                macro_rules! check_stop {
                    () => {
                        if store_count.upgrade().is_none() {
                            break;
                        }
                    };
                }

                tokio::select! {
                    notification = listener.try_recv() => {
                        check_stop!();
                        match notification {
                            Ok(Some(notification)) => {
                                update_worker_selectors(&mut query_worker_selectors);
                                if let Err(error) = handle_notification(notification, &db, &query_worker_selectors, &sender).await {
                                    tracing::error!(?error, "failed to process notification");
                                    let now = OffsetDateTime::now_utc();
                                    loop {
                                        check_stop!();
                                        match poll_all_events(&db, last_full_check, &query_worker_selectors).await {
                                            Ok(events) => {
                                                for event in events {
                                                    if sender.send(event).is_err() {
                                                        tracing::debug!("no listeners");
                                                        break;
                                                    }
                                                }
                                                break;
                                            }
                                            Err(error) => {
                                                tracing::error!(%error, "failed to update state");
                                            }
                                        }
                                    }
                                    last_full_check = now;
                                }
                            },
                            Ok(None) => {
                                tracing::warn!("connection lost, polling all events");
                                let now = OffsetDateTime::now_utc();
                                loop {
                                    check_stop!();
                                    update_worker_selectors(&mut query_worker_selectors);
                                    match poll_all_events(&db, last_full_check, &query_worker_selectors).await {
                                        Ok(events) => {
                                            for event in events {
                                                if sender.send(event).is_err() {
                                                    tracing::debug!("no listeners");
                                                    break;
                                                }
                                            }
                                            break;
                                        }
                                        Err(error) => {
                                            tracing::error!(%error, "failed to update state");
                                        }
                                    }
                                }
                                last_full_check = now;
                            }
                            Err(error) => {
                                tracing::error!(%error, "database error");
                                let now = OffsetDateTime::now_utc();
                                loop {
                                    check_stop!();
                                    update_worker_selectors(&mut query_worker_selectors);
                                    match poll_all_events(&db, last_full_check, &query_worker_selectors).await {
                                        Ok(events) => {
                                            for event in events {
                                                if sender.send(event).is_err() {
                                                    tracing::debug!("no listeners");
                                                    break;
                                                }
                                            }
                                            break;
                                        }
                                        Err(error) => {
                                            tracing::error!(%error, "failed to update state");
                                        }
                                    }
                                }
                                last_full_check = now;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        check_stop!();
                        let now = OffsetDateTime::now_utc();
                        loop {
                            check_stop!();
                            update_worker_selectors(&mut query_worker_selectors);
                            match poll_all_events(&db, last_full_check, &query_worker_selectors).await {
                                Ok(events) => {
                                    for event in events {
                                        if sender.send(event).is_err() {
                                            tracing::debug!("no listeners");
                                            break;
                                        }
                                    }
                                    break;
                                }
                                Err(error) => {
                                    tracing::error!(%error, "failed to update state");
                                }
                            }
                        }
                        last_full_check = now;
                    }
                }
            }
        });

        Ok(())
    }
}

async fn handle_notification(
    notification: PgNotification,
    db: &PgPool,
    worker_selectors: &[Json<WorkerSelector>],
    events: &broadcast::Sender<DbEvent>,
) -> eyre::Result<()> {
    match notification.channel() {
        event::TASK_ADDED => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            let event = query(
                r#"--sql
                SELECT
                    "id",
                    "target",
                    "timeout_policy"
                FROM
                    "ora"."task"
                WHERE
                    "id" = $1 AND "active" AND "status" = 'pending'
                "#,
            )
            .bind(id)
            .try_map(|row| {
                Ok(DbEvent::SchedulerTaskStore(
                    SchedulerTaskStoreEvent::TaskAdded(PendingTask {
                        id: row.try_get("id")?,
                        target: row.try_get::<OffsetDateTime, _>("target")?.into(),
                        timeout: row.try_get::<Json<TimeoutPolicy>, _>("timeout_policy")?.0,
                    }),
                ))
            })
            .fetch_optional(db)
            .await?;

            if let Some(event) = event {
                events.send(event).ok();
            } else {
                tracing::debug!("failed to fetch specific row, unexpected db state");
            }
        }
        event::TASK_CANCELLED => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            events
                .send(DbEvent::SchedulerTaskStore(
                    SchedulerTaskStoreEvent::TaskCancelled(id),
                ))
                .ok();
            events
                .send(DbEvent::WorkerStore(
                    WorkerStoreEvent::TaskCancelled(id),
                ))
                .ok();
            events
                .send(DbEvent::SchedulerScheduleStore(
                    SchedulerScheduleStoreEvent::TaskFinished(id),
                ))
                .ok();
            events.send(DbEvent::TaskDone(id)).ok();
        }
        event::TASK_READY => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            let event = query(
                r#"--sql
                SELECT
                    "id",
                    "target",
                    "worker_selector",
                    "data_format",
                    "data_bytes",
                    "data_json",
                    "timeout_policy",
                    "labels"
                FROM
                    "ora"."task"
                JOIN UNNEST($2::JSONB[]) AS ws("expected_selector") ON
                        "worker_selector" @> "expected_selector"
                            AND "worker_selector" <@ "expected_selector"
                WHERE
                    "id" = $1 AND "active" AND "status" = 'ready'
                "#,
            )
            .bind(id)
            .bind(worker_selectors)
            .try_map(|row: PgRow| {
                Ok(DbEvent::WorkerStore(WorkerStoreEvent::TaskReady(
                    ReadyTask {
                        id: row.try_get("id")?,
                        definition: task_definition_from_row(row)?,
                    },
                )))
            })
            .fetch_optional(db)
            .await?;

            if let Some(event) = event {
                events.send(event).ok();
            } else {
                tracing::debug!(
                    "failed to fetch task, unexpected db state or worker selectors did not match"
                );
            }
        }
        event::TASK_SUCCEEDED | event::TASK_FAILED => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            events
                .send(DbEvent::SchedulerScheduleStore(
                    SchedulerScheduleStoreEvent::TaskFinished(id),
                ))
                .ok();
            events.send(DbEvent::TaskDone(id)).ok();
        }
        event::SCHEDULE_ADDED => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            let event = query(
                r#"--sql
                SELECT
                    "id",
                    "schedule_policy",
                    "immediate",
                    "missed_tasks_policy",
                    "new_task",
                    "labels"
                FROM
                    "ora"."schedule"
                WHERE
                    "id" = $1 AND "active"
                "#,
            )
            .bind(id)
            .try_map(|row: PgRow| {
                Ok(DbEvent::SchedulerScheduleStore(
                    SchedulerScheduleStoreEvent::ScheduleAdded(Box::new(ActiveSchedule {
                        id: row.try_get("id")?,
                        definition: ScheduleDefinition {
                            policy: row.try_get::<Json<_>, _>("schedule_policy")?.0,
                            immediate: row.try_get("immediate")?,
                            missed_tasks: row.try_get::<Json<_>, _>("missed_tasks_policy")?.0,
                            new_task: row.try_get::<Json<_>, _>("new_task")?.0,
                            labels: row.try_get::<Json<_>, _>("labels")?.0,
                        },
                    })),
                ))
            })
            .fetch_optional(db)
            .await?;

            if let Some(event) = event {
                events.send(event).ok();
            } else {
                tracing::debug!("failed to fetch specific row, unexpected db state");
            }
        }
        event::SCHEDULE_CANCELLED => {
            let id: Uuid = notification
                .payload()
                .parse()
                .wrap_err("invalid notification payload")?;

            events
                .send(DbEvent::SchedulerScheduleStore(
                    SchedulerScheduleStoreEvent::ScheduleCancelled(id),
                ))
                .ok();
        }
        channel => {
            tracing::warn!(channel, "unknown channel");
        }
    }

    Ok(())
}

/// Poll states of all tasks and schedules,
/// this should only be used as a fallback mechanism.
#[tracing::instrument(skip_all)]
async fn poll_all_events(
    db: &PgPool,
    after: OffsetDateTime,
    worker_selectors: &[Json<WorkerSelector>],
) -> eyre::Result<Vec<DbEvent>> {
    let mut events = Vec::new();

    // We only want to read the database, with no
    // concurrent updates, so we can wait with reads if needed.
    //
    // We might still miss updates this way, so this
    // operation should not be considered as the only means of retrieving
    // changes.
    let mut tx = db.begin().await?;
    tx.execute(
        r#"--sql
        SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY DEFERRABLE;
        "#,
    )
    .await?;

    let new_events = query(
        r#"--sql
        SELECT
            "id",
            "target",
            "timeout_policy"
        FROM
            "ora"."task"
        WHERE
            "updated" >= $1 AND "active" AND "status" = 'pending'
        "#,
    )
    .bind(after)
    .try_map(|row: PgRow| {
        Ok(DbEvent::SchedulerTaskStore(
            SchedulerTaskStoreEvent::TaskAdded(PendingTask {
                id: row.try_get("id")?,
                target: row.try_get::<OffsetDateTime, _>("target")?.into(),
                timeout: row.try_get::<Json<TimeoutPolicy>, _>("timeout_policy")?.0,
            }),
        ))
    })
    .fetch_all(&mut *tx)
    .await?;
    events.extend(new_events);

    let new_events = query(
        r#"--sql
        SELECT
            "id"
        FROM
            "ora"."task"
        WHERE
            "updated" >= $1 AND "status" = 'cancelled'
        "#,
    )
    .bind(after)
    .try_map(|row: PgRow| {
        let id = row.try_get("id")?;
        Ok([
            DbEvent::SchedulerTaskStore(SchedulerTaskStoreEvent::TaskCancelled(id)),
            DbEvent::WorkerStore(WorkerStoreEvent::TaskCancelled(id)),
        ])
    })
    .fetch_all(&mut *tx)
    .await?;

    events.extend(new_events.into_iter().flatten());

    let new_events = query(
        r#"--sql
        SELECT
            "id",
            "schedule_policy",
            "immediate",
            "missed_tasks_policy",
            "new_task",
            "labels"
        FROM
            "ora"."schedule"
        WHERE
            "updated" >= $1 AND "active"
        "#,
    )
    .bind(after)
    .try_map(|row: PgRow| {
        Ok(DbEvent::SchedulerScheduleStore(
            SchedulerScheduleStoreEvent::ScheduleAdded(Box::new(ActiveSchedule {
                id: row.try_get("id")?,
                definition: ScheduleDefinition {
                    policy: row.try_get::<Json<_>, _>("schedule_policy")?.0,
                    immediate: row.try_get("immediate")?,
                    missed_tasks: row.try_get::<Json<_>, _>("missed_tasks_policy")?.0,
                    new_task: row.try_get::<Json<_>, _>("new_task")?.0,
                    labels: row.try_get::<Json<_>, _>("labels")?.0,
                },
            })),
        ))
    })
    .fetch_all(&mut *tx)
    .await?;

    events.extend(new_events);

    let new_events = query(
        r#"--sql
        SELECT
            "id"
        FROM
            "ora"."schedule"
        WHERE
            "updated" >= $1 AND NOT "active"
        "#,
    )
    .bind(after)
    .try_map(|row: PgRow| {
        Ok(DbEvent::SchedulerScheduleStore(
            SchedulerScheduleStoreEvent::ScheduleCancelled(row.try_get("id")?),
        ))
    })
    .fetch_all(&mut *tx)
    .await?;

    events.extend(new_events);

    let new_events = query(
        r#"--sql
        SELECT
            "id"
        FROM
            "ora"."task"
        WHERE
            "updated" >= $1 AND NOT "active"
        "#,
    )
    .bind(after)
    .try_map(|row: PgRow| {
        Ok(DbEvent::SchedulerScheduleStore(
            SchedulerScheduleStoreEvent::TaskFinished(row.try_get("id")?),
        ))
    })
    .fetch_all(&mut *tx)
    .await?;

    events.extend(new_events);

    let new_events = query(
        r#"--sql
        SELECT
            "id",
            "target",
            "worker_selector",
            "data_format",
            "data_bytes",
            "data_json",
            "timeout_policy",
            "labels"
        FROM
            "ora"."task"
        JOIN UNNEST($2::JSONB[]) AS ws("expected_selector") ON
                "worker_selector" @> "expected_selector"
                    AND "worker_selector" <@ "expected_selector"
        WHERE
            "updated" >= $1 AND "active" AND "status" = 'ready'
        "#,
    )
    .bind(after)
    .bind(worker_selectors)
    .try_map(|row: PgRow| {
        Ok(DbEvent::WorkerStore(WorkerStoreEvent::TaskReady(
            ReadyTask {
                id: row.try_get("id")?,
                definition: task_definition_from_row(row)?,
            },
        )))
    })
    .fetch_all(&mut *tx)
    .await?;

    events.extend(new_events);

    tx.commit().await?;

    Ok(events)
}
