//! Tests for the Ora backend implementations.
#![allow(clippy::missing_panics_doc, missing_docs)]

use std::{
    pin::pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::TryStreamExt;
use uuid::Uuid;

use crate::{
    Backend,
    common::{Label, TimeRange},
    executions::{
        ExecutionId, ExecutionStatus, FailedExecution, RetriedExecution, StartedExecution,
        SucceededExecution,
    },
    jobs::{
        JobDefinition, JobDetails, JobFilters, JobOrderBy, JobTypeId, NewJob, RetryPolicy,
        TimeoutPolicy,
    },
    schedules::{MissedTimePolicy, SchedulingPolicy},
};

/// Run basic smoke tests for the given backend.
pub async fn smoke(backend: &impl Backend) {
    job_execution(backend).await;
    job_cancellation(backend).await;
}

/// Smoke tests for jobs and executions.
async fn job_execution(backend: &impl Backend) {
    let job_definitions = [
        JobDefinition {
            job_type_id: JobTypeId::new("DoSomething").unwrap(),
            target_execution_time: std::time::SystemTime::now(),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: [Label {
                key: "foo_id".to_string(),
                value: "2".to_string(),
            }]
            .to_vec(),
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        },
        JobDefinition {
            job_type_id: JobTypeId::new("DoSomething2").unwrap(),
            target_execution_time: std::time::SystemTime::now(),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: [
                Label {
                    key: "a_id".to_string(),
                    value: "3".to_string(),
                },
                Label {
                    key: "b_id".to_string(),
                    value: "4".to_string(),
                },
            ]
            .to_vec(),
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::TargetExecutionTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        },
    ]
    .to_vec();
    let result = backend
        .add_jobs(&new_jobs(&job_definitions), None)
        .await
        .expect("Failed to add jobs");
    assert_eq!(result.job_ids().len(), job_definitions.len());

    let jobs = backend
        .list_jobs(crate::jobs::JobFilters::default(), None, 10, None)
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(jobs.len(), job_definitions.len());

    for mut job in jobs {
        let job_definition = job_definitions
            .iter()
            .find(|def| def.job_type_id == job.job.job_type_id)
            .expect("Job definition not found");

        job.job.labels.sort_by(|a, b| a.key.cmp(&b.key));

        assert_eq!(
            job.job.input_payload_json,
            job_definition.input_payload_json
        );
        assert_eq!(job.job.labels, job_definition.labels);
        assert_eq!(
            job.job.timeout_policy.timeout,
            job_definition.timeout_policy.timeout
        );
        assert_eq!(
            job.job.retry_policy.retries,
            job_definition.retry_policy.retries
        );
    }

    let ready_executions = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready_executions.len(), job_definitions.len());
    assert!(ready_executions.windows(2).all(|w| {
        w[0].priority > w[1].priority
            || (w[0].priority == w[1].priority && w[0].execution_id <= w[1].execution_id)
    }));

    backend
        .executions_started(
            &ready_executions
                .iter()
                .map(|exec| StartedExecution {
                    execution_id: exec.execution_id,
                    executor_id: Uuid::nil().into(),
                    started_at: std::time::SystemTime::now(),
                })
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();

    let ready_executions_2 = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap();
    assert!(ready_executions_2.is_none());

    let in_progress_executions = pin!(backend.in_progress_executions())
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(in_progress_executions.len(), job_definitions.len());

    let exec1 = &in_progress_executions[0];
    let exec2 = &in_progress_executions[1];

    backend
        .executions_succeeded(&[SucceededExecution {
            execution_id: exec1.execution_id,
            succeeded_at: std::time::SystemTime::now(),
            output_json: r#"{"result": "done"}"#.to_string(),
        }])
        .await
        .unwrap();

    let in_progress_executions = pin!(backend.in_progress_executions())
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(in_progress_executions.len(), 1);

    assert_eq!(in_progress_executions[0].execution_id, exec2.execution_id);

    backend
        .executions_retried(&[RetriedExecution {
            failed_execution: FailedExecution {
                execution_id: exec2.execution_id,
                job_id: exec2.job_id,
                failed_at: std::time::SystemTime::now(),
                failure_reason: "Temporary failure".to_string(),
            },
            retry_execution_time: SystemTime::now(),
        }])
        .await
        .unwrap();

    let ready_executions = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready_executions.len(), 1);

    backend
        .executions_failed(&[FailedExecution {
            execution_id: ready_executions[0].execution_id,
            job_id: ready_executions[0].job_id,
            failed_at: std::time::SystemTime::now(),
            failure_reason: "Permanent failure".to_string(),
        }])
        .await
        .unwrap();

    let in_progress_executions = pin!(backend.in_progress_executions())
        .try_next()
        .await
        .unwrap();
    assert!(in_progress_executions.is_none());

    let ready_executions = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap();
    assert!(ready_executions.is_none());
}

async fn job_cancellation(backend: &impl Backend) {
    let job_definitions = [JobDefinition {
        job_type_id: JobTypeId::new("ToBeCancelled").unwrap(),
        target_execution_time: std::time::SystemTime::now(),
        input_payload_json: r#"{"task": "clean"}"#.to_string(),
        labels: vec![],
        timeout_policy: TimeoutPolicy {
            timeout: Duration::from_secs(20),
            base_time: crate::jobs::TimeoutBaseTime::StartTime,
        },
        retry_policy: RetryPolicy {
            retries: 0,
            ..Default::default()
        },
        priority: 0,
    }]
    .to_vec();
    let result = backend
        .add_jobs(&new_jobs(&job_definitions), None)
        .await
        .expect("Failed to add jobs");
    assert_eq!(result.job_ids().len(), job_definitions.len());

    let ready_executions = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready_executions.len(), job_definitions.len());

    let cancelled_jobs = backend.cancel_jobs(JobFilters::default()).await.unwrap();

    assert_eq!(cancelled_jobs.len(), job_definitions.len());

    let ready_executions = pin!(backend.ready_executions(&[]))
        .try_next()
        .await
        .unwrap();
    assert!(ready_executions.is_none());

    let jobs = backend
        .list_jobs(
            JobFilters {
                execution_statuses: Some(vec![ExecutionStatus::Cancelled]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .unwrap()
        .0;

    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job.job_type_id, job_definitions[0].job_type_id);
    assert_eq!(jobs[0].executions.len(), 1);
    assert_eq!(jobs[0].executions[0].status, ExecutionStatus::Cancelled);

    backend
        .executions_succeeded(&[SucceededExecution {
            execution_id: jobs[0].executions[0].id,
            succeeded_at: std::time::SystemTime::now(),
            output_json: r#"{"result": "should not happen"}"#.to_string(),
        }])
        .await
        .unwrap();

    let jobs = backend
        .list_jobs(
            JobFilters {
                execution_statuses: Some(vec![ExecutionStatus::Cancelled]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .unwrap()
        .0;

    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job.job_type_id, job_definitions[0].job_type_id);
    assert_eq!(jobs[0].executions.len(), 1);
    assert_eq!(jobs[0].executions[0].status, ExecutionStatus::Cancelled);
}

pub async fn job_queries(backend: &impl Backend) {
    let job_definitions = [
        JobDefinition {
            job_type_id: JobTypeId::new("QueryJob1").unwrap(),
            target_execution_time: std::time::SystemTime::now(),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: [Label {
                key: "foo".to_string(),
                value: "bar".to_string(),
            }]
            .to_vec(),
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        },
        JobDefinition {
            job_type_id: JobTypeId::new("QueryJob2").unwrap(),
            target_execution_time: std::time::SystemTime::now(),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: [Label {
                key: "foo".to_string(),
                value: "bar".to_string(),
            }]
            .to_vec(),
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        },
        JobDefinition {
            job_type_id: JobTypeId::new("QueryJob2").unwrap(),
            target_execution_time: UNIX_EPOCH + Duration::from_secs(5),
            input_payload_json: r#"{"task": "build"}"#.to_string(),
            labels: [
                Label {
                    key: "foo".to_string(),
                    value: "baz".to_string(),
                },
                Label {
                    key: "bar".to_string(),
                    value: "stuff".to_string(),
                },
            ]
            .to_vec(),
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(30),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 1,
                ..Default::default()
            },
            priority: 0,
        },
    ]
    .to_vec();
    let result = backend
        .add_jobs(&new_jobs(&job_definitions), None)
        .await
        .expect("Failed to add jobs");
    assert_eq!(result.job_ids().len(), job_definitions.len());

    let jobs = backend
        .list_jobs(crate::jobs::JobFilters::default(), None, 10, None)
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(jobs.len(), job_definitions.len());

    let foo_bar_jobs = backend
        .list_jobs(
            JobFilters {
                labels: Some(vec![crate::common::LabelFilter {
                    key: "foo".to_string(),
                    value: Some("bar".to_string()),
                }]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(foo_bar_jobs.len(), 2);

    let foo_bar_jobs = backend
        .list_jobs(
            JobFilters {
                labels: Some(vec![crate::common::LabelFilter {
                    key: "foo".to_string(),
                    value: Some("bar".to_string()),
                }]),
                job_type_ids: Some(vec![JobTypeId::new("QueryJob1").unwrap()]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(foo_bar_jobs.len(), 1);

    let foo_jobs = backend
        .list_jobs(
            JobFilters {
                labels: Some(vec![crate::common::LabelFilter {
                    key: "foo".to_string(),
                    value: None,
                }]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(foo_jobs.len(), 3);

    let foo_bar_jobs = backend
        .list_jobs(
            JobFilters {
                labels: Some(vec![
                    crate::common::LabelFilter {
                        key: "foo".to_string(),
                        value: None,
                    },
                    crate::common::LabelFilter {
                        key: "foo".to_string(),
                        value: Some("bar".to_string()),
                    },
                ]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(foo_bar_jobs.len(), 2);

    let jobs = backend
        .list_jobs(
            JobFilters {
                target_execution_time: Some(TimeRange {
                    start: Some(UNIX_EPOCH),
                    end: Some(UNIX_EPOCH + Duration::from_secs(10)),
                }),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .expect("Failed to list jobs")
        .0;

    assert_eq!(jobs.len(), 1);
}

pub async fn pagination_and_ordering(backend: &impl Backend) {
    let job_definitions = (0..25)
        .map(|i| JobDefinition {
            job_type_id: JobTypeId::new(format!("Job{i}")).unwrap(),
            target_execution_time: UNIX_EPOCH + Duration::from_secs(200) - Duration::from_secs(i),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        })
        .collect::<Vec<_>>();
    let result = backend
        .add_jobs(&new_jobs(&job_definitions), None)
        .await
        .expect("Failed to add jobs");
    assert_eq!(result.job_ids().len(), job_definitions.len());

    {
        let mut all_jobs = Vec::new();
        let mut next_page_token = None;

        loop {
            let (jobs, page_token) = backend
                .list_jobs(crate::jobs::JobFilters::default(), None, 2, next_page_token)
                .await
                .expect("Failed to list jobs");
            assert!(jobs.len() <= 2);
            all_jobs.extend(jobs);
            if let Some(token) = page_token {
                next_page_token = Some(token);
            } else {
                break;
            }
        }
        assert_eq!(all_jobs.len(), job_definitions.len());
    }

    {
        let mut all_jobs = Vec::new();
        let mut next_page_token = None;

        loop {
            let (jobs, page_token) = backend
                .list_jobs(
                    crate::jobs::JobFilters::default(),
                    Some(crate::jobs::JobOrderBy::CreatedAtAsc),
                    2,
                    next_page_token,
                )
                .await
                .expect("Failed to list jobs");
            assert!(jobs.len() <= 2);
            all_jobs.extend(jobs);
            if let Some(token) = page_token {
                next_page_token = Some(token);
            } else {
                break;
            }
        }
        assert_eq!(all_jobs.len(), job_definitions.len());

        let reference_jobs = job_definitions.clone();
        for (i, job) in all_jobs.iter().enumerate() {
            assert_eq!(job.job.job_type_id, reference_jobs[i].job_type_id);
        }
    }

    {
        let mut all_jobs = Vec::new();
        let mut next_page_token = None;

        loop {
            let (jobs, page_token) = backend
                .list_jobs(
                    crate::jobs::JobFilters::default(),
                    Some(crate::jobs::JobOrderBy::CreatedAtDesc),
                    2,
                    next_page_token,
                )
                .await
                .expect("Failed to list jobs");
            assert!(jobs.len() <= 2);
            all_jobs.extend(jobs);
            if let Some(token) = page_token {
                next_page_token = Some(token);
            } else {
                break;
            }
        }
        assert_eq!(all_jobs.len(), job_definitions.len());

        let mut reference_jobs = job_definitions.clone();
        reference_jobs.reverse();
        for (i, job) in all_jobs.iter().enumerate() {
            assert_eq!(job.job.job_type_id, reference_jobs[i].job_type_id);
        }
    }

    {
        let mut all_jobs = Vec::new();
        let mut next_page_token = None;

        loop {
            let (jobs, page_token) = backend
                .list_jobs(
                    crate::jobs::JobFilters::default(),
                    Some(crate::jobs::JobOrderBy::TargetExecutionTimeDesc),
                    2,
                    next_page_token,
                )
                .await
                .expect("Failed to list jobs");
            assert!(jobs.len() <= 2);
            all_jobs.extend(jobs);
            if let Some(token) = page_token {
                next_page_token = Some(token);
            } else {
                break;
            }
        }
        assert_eq!(all_jobs.len(), job_definitions.len());

        let reference_jobs = job_definitions.clone();
        for (i, job) in all_jobs.iter().enumerate() {
            assert_eq!(job.job.job_type_id, reference_jobs[i].job_type_id);
        }
    }

    {
        let all_jobs = list_all_jobs(backend, JobOrderBy::TargetExecutionTimeAsc, 2).await;
        assert_eq!(all_jobs.len(), job_definitions.len());

        let mut reference_jobs = job_definitions.clone();
        reference_jobs.reverse();
        for (i, job) in all_jobs.iter().enumerate() {
            assert_eq!(job.job.job_type_id, reference_jobs[i].job_type_id);
        }
    }

    // Target times that are not in creation order and have ties,
    // pages must continue right after the last job of the previous page.
    let tied_definitions = (0..12)
        .map(|i| JobDefinition {
            job_type_id: JobTypeId::new(format!("Tied{i}")).unwrap(),
            target_execution_time: UNIX_EPOCH + Duration::from_secs(1000 + (i * 7) % 5),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        })
        .collect::<Vec<_>>();
    backend
        .add_jobs(&new_jobs(&tied_definitions), None)
        .await
        .expect("Failed to add jobs");

    let all_jobs = list_all_jobs(backend, JobOrderBy::CreatedAtAsc, 100).await;
    assert_eq!(
        all_jobs.len(),
        job_definitions.len() + tied_definitions.len()
    );

    for order_by in [
        JobOrderBy::TargetExecutionTimeAsc,
        JobOrderBy::TargetExecutionTimeDesc,
    ] {
        let mut expected = all_jobs
            .iter()
            .map(|job| (job.job.target_execution_time, job.id.0))
            .collect::<Vec<_>>();

        // Ties are always ordered by ascending IDs.
        match order_by {
            JobOrderBy::TargetExecutionTimeDesc => {
                expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            _ => expected.sort(),
        }

        let paged = list_all_jobs(backend, order_by.clone(), 2)
            .await
            .iter()
            .map(|job| (job.job.target_execution_time, job.id.0))
            .collect::<Vec<_>>();

        assert_eq!(
            paged, expected,
            "unexpected jobs when paging by {order_by:?}"
        );
    }
}

/// Lists all jobs by following the page tokens.
async fn list_all_jobs(
    backend: &impl Backend,
    order_by: JobOrderBy,
    page_size: u32,
) -> Vec<JobDetails> {
    let mut all_jobs = Vec::new();
    let mut next_page_token = None;

    loop {
        let (jobs, page_token) = backend
            .list_jobs(
                JobFilters::default(),
                Some(order_by.clone()),
                page_size,
                next_page_token,
            )
            .await
            .expect("Failed to list jobs");
        assert!(jobs.len() <= page_size as usize);
        all_jobs.extend(jobs);

        match page_token {
            Some(token) => next_page_token = Some(token),
            None => break,
        }
    }

    all_jobs
}

pub async fn schedules(backend: &impl Backend) {
    use crate::schedules::{ScheduleDefinition, ScheduleFilters};

    let schedule_definitions = [ScheduleDefinition {
        job_template: JobDefinition {
            job_type_id: JobTypeId::new("ScheduledJob1").unwrap(),
            target_execution_time: std::time::SystemTime::now(),
            input_payload_json: r#"{"task": "clean"}"#.to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 0,
                ..Default::default()
            },
            priority: 0,
        },
        scheduling: SchedulingPolicy::FixedInterval {
            interval: Duration::from_secs(1),
            immediate: true,
            missed: MissedTimePolicy::Skip,
        },
        labels: vec![],
        time_range: TimeRange::default(),
    }]
    .to_vec();

    let result = backend
        .add_schedules(&schedule_definitions, None)
        .await
        .expect("Failed to add schedules");
    assert_eq!(result.schedule_ids().len(), schedule_definitions.len());

    let schedules = backend
        .list_schedules(ScheduleFilters::default(), None, 10, None)
        .await
        .expect("Failed to list schedules")
        .0;

    assert_eq!(schedules.len(), schedule_definitions.len());

    let pending_schedules = pin!(backend.pending_schedules())
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending_schedules.len(), schedule_definitions.len());

    for schedule in pending_schedules {
        backend
            .add_jobs(
                &[NewJob {
                    job: schedule.job_template.clone(),
                    schedule_id: Some(schedule.schedule_id),
                }],
                None,
            )
            .await
            .unwrap();
    }

    let pending_schedules = pin!(backend.pending_schedules()).try_next().await.unwrap();
    assert!(pending_schedules.is_none());
}

/// Tests for counting jobs by status, and jobs and schedules by job type.
pub async fn counts(backend: &impl Backend) {
    use crate::schedules::{ScheduleDefinition, ScheduleFilters, ScheduleStatus};
    use ExecutionStatus::{Cancelled, Failed, InProgress, Pending, Succeeded};

    fn job(job_type_id: &str) -> JobDefinition {
        JobDefinition {
            job_type_id: JobTypeId::new(job_type_id).unwrap(),
            target_execution_time: SystemTime::now(),
            input_payload_json: "{}".to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::from_secs(20),
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 1,
                ..Default::default()
            },
            priority: 0,
        }
    }

    async fn start(backend: &impl Backend, jobs: &[crate::jobs::JobId]) -> Vec<ExecutionId> {
        let ready = pin!(backend.ready_executions(&[]))
            .try_next()
            .await
            .unwrap()
            .unwrap();

        let started = ready
            .iter()
            .filter(|execution| jobs.contains(&execution.job_id))
            .map(|execution| StartedExecution {
                execution_id: execution.execution_id,
                executor_id: Uuid::nil().into(),
                started_at: SystemTime::now(),
            })
            .collect::<Vec<_>>();
        assert_eq!(started.len(), jobs.len());

        backend.executions_started(&started).await.unwrap();

        jobs.iter()
            .map(|job| {
                ready
                    .iter()
                    .find(|execution| execution.job_id == *job)
                    .unwrap()
                    .execution_id
            })
            .collect()
    }

    let job_ids = backend
        .add_jobs(
            &new_jobs(&[
                job("CountA"),
                job("CountA"),
                job("CountA"),
                job("CountA"),
                job("CountA"),
                job("CountB"),
                job("CountB"),
                job("CountB"),
                job("CountB"),
            ]),
            None,
        )
        .await
        .unwrap()
        .job_ids()
        .to_vec();
    let [a1, a2, a3, a4, _a5, b1, b2, _b3, b4] = job_ids[..] else {
        panic!("unexpected number of jobs");
    };

    // a1, a2: succeeded, a3: failed, a4: in progress, a5: pending,
    // b1: retried and in progress, b2: cancelled, b3: pending,
    // b4: retried and cancelled.
    let started = start(backend, &[a1, a2, a3, a4, b1, b2, b4]).await;

    backend
        .executions_succeeded(
            &started[..2]
                .iter()
                .map(|execution_id| SucceededExecution {
                    execution_id: *execution_id,
                    succeeded_at: SystemTime::now(),
                    output_json: "{}".to_string(),
                })
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();

    backend
        .executions_failed(&[FailedExecution {
            execution_id: started[2],
            job_id: a3,
            failed_at: SystemTime::now(),
            failure_reason: "failed".to_string(),
        }])
        .await
        .unwrap();

    backend
        .executions_retried(
            &[(started[4], b1), (started[6], b4)].map(|(execution_id, job_id)| RetriedExecution {
                failed_execution: FailedExecution {
                    execution_id,
                    job_id,
                    failed_at: SystemTime::now(),
                    failure_reason: "retried".to_string(),
                },
                retry_execution_time: SystemTime::now(),
            }),
        )
        .await
        .unwrap();

    start(backend, &[b1]).await;

    let cancelled = backend
        .cancel_jobs(JobFilters {
            job_ids: Some(vec![b2, b4]),
            ..Default::default()
        })
        .await
        .unwrap();
    // Each job is cancelled once, even with earlier failed executions.
    assert_eq!(cancelled.len(), 2);

    let b4_details = backend
        .list_jobs(
            JobFilters {
                job_ids: Some(vec![b4]),
                ..Default::default()
            },
            None,
            10,
            None,
        )
        .await
        .unwrap()
        .0;
    assert_eq!(
        b4_details[0]
            .executions
            .iter()
            .map(|execution| execution.status)
            .collect::<Vec<_>>(),
        [ExecutionStatus::Failed, ExecutionStatus::Cancelled]
    );

    let count = async |statuses: &[ExecutionStatus]| {
        backend
            .count_jobs(JobFilters {
                execution_statuses: Some(statuses.to_vec()),
                ..Default::default()
            })
            .await
            .unwrap()
    };

    assert_eq!(count(&[Pending]).await, 2);
    assert_eq!(count(&[InProgress]).await, 2);
    assert_eq!(count(&[Succeeded]).await, 2);
    assert_eq!(count(&[Failed]).await, 1);
    assert_eq!(count(&[Cancelled]).await, 2);
    assert_eq!(count(&[Pending, InProgress]).await, 4);
    assert_eq!(count(&[Succeeded, Failed, Cancelled]).await, 5);
    assert_eq!(count(&[Succeeded, Cancelled]).await, 4);
    assert_eq!(count(&[Pending, Failed]).await, 3);
    assert_eq!(count(&[Pending, InProgress, Failed]).await, 5);
    assert_eq!(
        count(&[Pending, InProgress, Succeeded, Failed, Cancelled]).await,
        9
    );
    assert_eq!(count(&[]).await, 0);

    // Jobs with multiple executions of the executor are counted once.
    assert_eq!(
        backend
            .count_jobs(JobFilters {
                executor_ids: Some(vec![Uuid::nil().into()]),
                ..Default::default()
            })
            .await
            .unwrap(),
        7
    );

    // Counts of a job type with a status, e.g. for showing counts by job type.
    let type_count = async |job_type_id: &str, statuses: &[ExecutionStatus]| {
        backend
            .count_jobs(JobFilters {
                job_type_ids: Some(vec![JobTypeId::new(job_type_id).unwrap()]),
                execution_statuses: Some(statuses.to_vec()),
                ..Default::default()
            })
            .await
            .unwrap()
    };

    assert_eq!(type_count("CountA", &[Pending]).await, 1);
    assert_eq!(type_count("CountA", &[InProgress]).await, 1);
    assert_eq!(type_count("CountA", &[Succeeded]).await, 2);
    assert_eq!(type_count("CountA", &[Failed]).await, 1);
    assert_eq!(type_count("CountA", &[Cancelled]).await, 0);
    assert_eq!(type_count("CountB", &[Pending]).await, 1);
    assert_eq!(type_count("CountB", &[InProgress]).await, 1);
    assert_eq!(type_count("CountB", &[Succeeded, Failed]).await, 0);
    assert_eq!(type_count("CountB", &[InProgress, Cancelled]).await, 3);

    let schedule = |job_type_id: &str| ScheduleDefinition {
        job_template: job(job_type_id),
        scheduling: SchedulingPolicy::FixedInterval {
            interval: Duration::from_secs(1),
            immediate: true,
            missed: MissedTimePolicy::Skip,
        },
        labels: vec![],
        time_range: TimeRange::default(),
    };

    let schedule_ids = backend
        .add_schedules(
            &[schedule("CountA"), schedule("CountA"), schedule("CountB")],
            None,
        )
        .await
        .unwrap()
        .schedule_ids()
        .to_vec();

    backend
        .stop_schedules(ScheduleFilters {
            schedule_ids: Some(vec![schedule_ids[0]]),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(
        backend
            .count_schedules(ScheduleFilters {
                job_type_ids: Some(vec![JobTypeId::new("CountA").unwrap()]),
                statuses: Some(vec![ScheduleStatus::Active]),
                ..Default::default()
            })
            .await
            .unwrap(),
        1
    );

    assert_eq!(
        backend
            .count_schedules(ScheduleFilters {
                statuses: Some(vec![ScheduleStatus::Active, ScheduleStatus::Stopped]),
                ..Default::default()
            })
            .await
            .unwrap(),
        3
    );
}

/// Tests for edge cases of executions, cancellations and schedules.
pub async fn edge_cases(backend: &impl Backend) {
    use crate::{
        executors::ExecutorId,
        schedules::{ScheduleDefinition, ScheduleFilters},
    };

    fn job(job_type_id: &str) -> JobDefinition {
        JobDefinition {
            job_type_id: JobTypeId::new(job_type_id).unwrap(),
            target_execution_time: SystemTime::now(),
            input_payload_json: "{}".to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::ZERO,
                base_time: crate::jobs::TimeoutBaseTime::TargetExecutionTime,
            },
            retry_policy: RetryPolicy {
                retries: 1,
                ..Default::default()
            },
            priority: 0,
        }
    }

    let executor_id = ExecutorId(Uuid::nil());
    let other_executor_id = ExecutorId(Uuid::from_u128(1));

    // Cancelled executions are not started, and repeated calls
    // by the same executor return the same executions.
    {
        let job_ids = backend
            .add_jobs(&new_jobs(&[job("EdgeStart"), job("EdgeStart")]), None)
            .await
            .unwrap()
            .job_ids()
            .to_vec();

        let ready = pin!(backend.ready_executions(&[]))
            .try_next()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ready.len(), 2);

        let cancelled = backend
            .cancel_jobs(JobFilters {
                job_ids: Some(vec![job_ids[0]]),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(cancelled.len(), 1);

        let started = |executor_id| {
            ready
                .iter()
                .map(|execution| StartedExecution {
                    execution_id: execution.execution_id,
                    executor_id,
                    started_at: SystemTime::now(),
                })
                .collect::<Vec<_>>()
        };

        let not_cancelled = ready
            .iter()
            .find(|execution| execution.job_id == job_ids[1])
            .unwrap()
            .execution_id;

        let started_ids = backend
            .executions_started(&started(executor_id))
            .await
            .unwrap();
        assert_eq!(started_ids, vec![not_cancelled]);

        let started_ids = backend
            .executions_started(&started(executor_id))
            .await
            .unwrap();
        assert_eq!(started_ids, vec![not_cancelled]);

        let started_ids = backend
            .executions_started(&started(other_executor_id))
            .await
            .unwrap();
        assert!(started_ids.is_empty());

        // In-progress executions report the target time of the
        // job, not the execution, which differs for retries.
        let retry_execution_time = SystemTime::now() - Duration::from_hours(1);

        backend
            .executions_retried(&[RetriedExecution {
                failed_execution: FailedExecution {
                    execution_id: not_cancelled,
                    job_id: job_ids[1],
                    failed_at: SystemTime::now(),
                    failure_reason: "retry".to_string(),
                },
                retry_execution_time,
            }])
            .await
            .unwrap();

        let ready = pin!(backend.ready_executions(&[]))
            .try_next()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].attempt_number, 2);

        backend
            .executions_started(&[StartedExecution {
                execution_id: ready[0].execution_id,
                executor_id,
                started_at: SystemTime::now(),
            }])
            .await
            .unwrap();

        let in_progress = pin!(backend.in_progress_executions())
            .try_next()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(in_progress.len(), 1);

        let job_target_execution_time = backend
            .list_jobs(
                JobFilters {
                    job_ids: Some(vec![job_ids[1]]),
                    ..Default::default()
                },
                None,
                1,
                None,
            )
            .await
            .unwrap()
            .0[0]
            .job
            .target_execution_time;

        let difference = in_progress[0]
            .target_execution_time
            .duration_since(job_target_execution_time)
            .unwrap_or_else(|e| e.duration());
        assert!(difference < Duration::from_millis(1));

        backend
            .executions_succeeded(&[SucceededExecution {
                execution_id: ready[0].execution_id,
                succeeded_at: SystemTime::now(),
                output_json: "{}".to_string(),
            }])
            .await
            .unwrap();
    }

    // Cancelling finished jobs cancels nothing.
    {
        let job_ids = backend
            .add_jobs(&new_jobs(&[job("EdgeCancel")]), None)
            .await
            .unwrap()
            .job_ids()
            .to_vec();

        let cancelled = backend
            .cancel_jobs(JobFilters {
                execution_statuses: Some(vec![ExecutionStatus::Succeeded]),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(cancelled.is_empty());

        let cancelled = backend
            .cancel_jobs(JobFilters {
                job_ids: Some(job_ids.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(cancelled.len(), 1);
    }

    // Concurrent additions with the same `if_not_exists` filters add jobs once.
    {
        let filters = JobFilters {
            job_type_ids: Some(vec![JobTypeId::new("EdgeUnique").unwrap()]),
            ..Default::default()
        };
        let jobs = new_jobs(&[job("EdgeUnique")]);

        let results = futures::future::join_all(
            (0..8).map(|_| backend.add_jobs(&jobs, Some(filters.clone()))),
        )
        .await;

        let added = results
            .into_iter()
            .map(Result::unwrap)
            .filter(|result| !result.added_job_ids().is_empty())
            .count();
        assert_eq!(added, 1);
        assert_eq!(backend.count_jobs(filters).await.unwrap(), 1);
    }

    // Jobs are not added for stopped schedules.
    {
        let schedule_ids = backend
            .add_schedules(
                &[ScheduleDefinition {
                    job_template: job("EdgeScheduled"),
                    scheduling: SchedulingPolicy::FixedInterval {
                        interval: Duration::from_mins(1),
                        immediate: false,
                        missed: MissedTimePolicy::Skip,
                    },
                    labels: vec![],
                    time_range: TimeRange::default(),
                }],
                None,
            )
            .await
            .unwrap()
            .schedule_ids()
            .to_vec();

        let schedule_filters = ScheduleFilters {
            schedule_ids: Some(schedule_ids.clone()),
            ..Default::default()
        };

        backend
            .stop_schedules(schedule_filters.clone())
            .await
            .unwrap();

        let schedule_job = |schedule_id| NewJob {
            job: job("EdgeScheduled"),
            schedule_id: Some(schedule_id),
        };

        let added = backend
            .add_jobs(&[schedule_job(schedule_ids[0])], None)
            .await
            .unwrap();
        assert!(added.job_ids().is_empty());

        // Only one job is added for an active schedule.
        let schedule_ids = backend
            .add_schedules(
                &[ScheduleDefinition {
                    job_template: job("EdgeScheduled"),
                    scheduling: SchedulingPolicy::FixedInterval {
                        interval: Duration::from_mins(1),
                        immediate: false,
                        missed: MissedTimePolicy::Skip,
                    },
                    labels: vec![],
                    time_range: TimeRange::default(),
                }],
                None,
            )
            .await
            .unwrap()
            .schedule_ids()
            .to_vec();

        let added = backend
            .add_jobs(
                &[schedule_job(schedule_ids[0]), schedule_job(schedule_ids[0])],
                None,
            )
            .await
            .unwrap();
        assert_eq!(added.job_ids().len(), 1);

        let added = backend
            .add_jobs(&[schedule_job(schedule_ids[0])], None)
            .await
            .unwrap();
        assert!(added.job_ids().is_empty());

        assert_eq!(
            backend
                .count_jobs(JobFilters {
                    job_type_ids: Some(vec![JobTypeId::new("EdgeScheduled").unwrap()]),
                    ..Default::default()
                })
                .await
                .unwrap(),
            1
        );
    }

    // Failure reasons are arbitrary text, they can contain NUL characters.
    {
        let job_id = backend
            .add_jobs(&new_jobs(&[job("EdgeNul")]), None)
            .await
            .unwrap()
            .job_ids()[0];

        let execution_id = pin!(backend.ready_executions(&[]))
            .try_next()
            .await
            .unwrap()
            .unwrap()
            .into_iter()
            .find(|execution| execution.job_id == job_id)
            .unwrap()
            .execution_id;

        backend
            .executions_started(&[StartedExecution {
                execution_id,
                executor_id,
                started_at: SystemTime::now(),
            }])
            .await
            .unwrap();

        backend
            .executions_failed(&[FailedExecution {
                execution_id,
                job_id,
                failed_at: SystemTime::now(),
                failure_reason: "before\0after".to_string(),
            }])
            .await
            .unwrap();

        let job = backend
            .list_jobs(
                JobFilters {
                    job_ids: Some(vec![job_id]),
                    ..Default::default()
                },
                None,
                1,
                None,
            )
            .await
            .unwrap()
            .0
            .remove(0);

        let execution = job.executions.last().unwrap();
        assert_eq!(execution.status, ExecutionStatus::Failed);

        let failure_reason = execution.failure_reason.as_deref().unwrap();
        assert!(failure_reason.starts_with("before"));
        assert!(failure_reason.ends_with("after"));
    }
}

/// Tests for job priorities.
pub async fn priorities(backend: &impl Backend) {
    use std::collections::HashMap;

    use crate::{
        executions::ReadyExecution,
        executors::ExecutorId,
        schedules::{ScheduleDefinition, ScheduleFilters},
    };

    fn job(priority: i32) -> JobDefinition {
        JobDefinition {
            job_type_id: JobTypeId::new("Prioritized").unwrap(),
            target_execution_time: SystemTime::now() - Duration::from_secs(1),
            input_payload_json: "{}".to_string(),
            labels: vec![],
            timeout_policy: TimeoutPolicy {
                timeout: Duration::ZERO,
                base_time: crate::jobs::TimeoutBaseTime::StartTime,
            },
            retry_policy: RetryPolicy {
                retries: 1,
                ..Default::default()
            },
            priority,
        }
    }

    async fn all_ready_executions(backend: &impl Backend) -> Vec<ReadyExecution> {
        ignored_ready_executions(backend, &[]).await
    }

    async fn ignored_ready_executions(
        backend: &impl Backend,
        ignore: &[ExecutionId],
    ) -> Vec<ReadyExecution> {
        pin!(backend.ready_executions(ignore))
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .concat()
    }

    /// Ready executions must be ordered by priority descending, then by ID.
    fn assert_ready_order(ready: &[ReadyExecution]) {
        assert!(
            ready.windows(2).all(|w| {
                w[0].priority > w[1].priority
                    || (w[0].priority == w[1].priority && w[0].execution_id < w[1].execution_id)
            }),
            "ready executions are not ordered by priority and ID"
        );
    }

    let executor_id = ExecutorId(Uuid::nil());

    // The extremes must not overflow anywhere.
    let priorities = [0, 10, -5, 10, i32::MIN, i32::MAX];

    let job_ids = backend
        .add_jobs(&new_jobs(&priorities.map(job)), None)
        .await
        .unwrap()
        .job_ids()
        .to_vec();

    let job_priorities = job_ids
        .iter()
        .copied()
        .zip(priorities)
        .collect::<HashMap<_, _>>();

    let ready = all_ready_executions(backend).await;
    assert_eq!(ready.len(), priorities.len());
    assert_ready_order(&ready);
    assert_eq!(ready[0].priority, i32::MAX);
    assert_eq!(ready[ready.len() - 1].priority, i32::MIN);

    for execution in &ready {
        assert_eq!(execution.priority, job_priorities[&execution.job_id]);
    }

    // Ignored executions are not returned.
    {
        let ignored = [ready[0].execution_id, ready[3].execution_id];
        let not_ignored = ignored_ready_executions(backend, &ignored).await;
        assert_eq!(not_ignored.len(), ready.len() - ignored.len());
        assert!(
            not_ignored
                .iter()
                .all(|execution| !ignored.contains(&execution.execution_id))
        );
        assert_ready_order(&not_ignored);
    }

    // Jobs with equal priority are ordered by creation.
    let tied = ready
        .iter()
        .filter(|execution| execution.priority == 10)
        .map(|execution| execution.job_id)
        .collect::<Vec<_>>();
    assert_eq!(tied, vec![job_ids[1], job_ids[3]]);

    // Listing by priority, ties are ordered by ascending IDs.
    for order_by in [JobOrderBy::PriorityAsc, JobOrderBy::PriorityDesc] {
        let mut expected = job_ids
            .iter()
            .map(|id| (job_priorities[id], id.0))
            .collect::<Vec<_>>();

        match order_by {
            JobOrderBy::PriorityDesc => {
                expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            _ => expected.sort(),
        }

        let paged = list_all_jobs(backend, order_by.clone(), 2)
            .await
            .iter()
            .map(|job| (job.job.priority, job.id.0))
            .collect::<Vec<_>>();

        assert_eq!(
            paged, expected,
            "unexpected jobs when paging by {order_by:?}"
        );
    }

    // Filtering by priority.
    for (min_priority, max_priority, expected) in [
        (Some(0), None, 4),
        (None, Some(0), 3),
        (Some(-5), Some(10), 4),
        (Some(10), Some(10), 2),
        (Some(11), Some(10), 0),
        (Some(i32::MIN), Some(i32::MAX), 6),
    ] {
        let filters = JobFilters {
            min_priority,
            max_priority,
            ..Default::default()
        };

        assert_eq!(
            backend.count_jobs(filters.clone()).await.unwrap(),
            expected,
            "unexpected count for priorities {min_priority:?}..={max_priority:?}"
        );

        let (jobs, _) = backend.list_jobs(filters, None, 100, None).await.unwrap();
        assert_eq!(jobs.len() as u64, expected);
        assert!(jobs.iter().all(|job| {
            min_priority.is_none_or(|min| job.job.priority >= min)
                && max_priority.is_none_or(|max| job.job.priority <= max)
        }));
    }

    // Retried executions keep the priority of the job.
    {
        let execution = ready
            .iter()
            .find(|execution| execution.job_id == job_ids[2])
            .unwrap();

        backend
            .executions_started(&[StartedExecution {
                execution_id: execution.execution_id,
                executor_id,
                started_at: SystemTime::now(),
            }])
            .await
            .unwrap();

        backend
            .executions_retried(&[RetriedExecution {
                failed_execution: FailedExecution {
                    job_id: execution.job_id,
                    execution_id: execution.execution_id,
                    failed_at: SystemTime::now(),
                    failure_reason: "retry".to_string(),
                },
                retry_execution_time: SystemTime::now() - Duration::from_secs(1),
            }])
            .await
            .unwrap();

        let ready = all_ready_executions(backend).await;
        assert_eq!(ready.len(), priorities.len());
        assert_ready_order(&ready);

        let retried = ready
            .iter()
            .find(|e| e.job_id == job_ids[2])
            .expect("retried execution is not ready");
        assert_ne!(retried.execution_id, execution.execution_id);
        assert_eq!(retried.attempt_number, 2);
        assert_eq!(retried.priority, -5);
    }

    // Jobs created by schedules get the priority of the template.
    {
        let schedule_ids = backend
            .add_schedules(
                &[ScheduleDefinition {
                    job_template: JobDefinition {
                        job_type_id: JobTypeId::new("PrioritizedScheduled").unwrap(),
                        ..job(7)
                    },
                    scheduling: SchedulingPolicy::FixedInterval {
                        interval: Duration::from_mins(1),
                        immediate: true,
                        missed: MissedTimePolicy::Skip,
                    },
                    labels: vec![],
                    time_range: TimeRange::default(),
                }],
                None,
            )
            .await
            .unwrap()
            .schedule_ids()
            .to_vec();

        let (schedules, _) = backend
            .list_schedules(
                ScheduleFilters {
                    schedule_ids: Some(schedule_ids.clone()),
                    ..Default::default()
                },
                None,
                10,
                None,
            )
            .await
            .unwrap();
        assert_eq!(schedules[0].schedule.job_template.priority, 7);

        let pending_schedule = pin!(backend.pending_schedules())
            .try_next()
            .await
            .unwrap()
            .unwrap()
            .into_iter()
            .find(|schedule| schedule.schedule_id == schedule_ids[0])
            .unwrap();
        assert_eq!(pending_schedule.job_template.priority, 7);

        backend
            .add_jobs(
                &[NewJob {
                    job: pending_schedule.job_template,
                    schedule_id: Some(schedule_ids[0]),
                }],
                None,
            )
            .await
            .unwrap();

        let (jobs, _) = backend
            .list_jobs(
                JobFilters {
                    schedule_ids: Some(schedule_ids),
                    ..Default::default()
                },
                None,
                10,
                None,
            )
            .await
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job.priority, 7);
    }

    // The ordering holds across batches of ready executions.
    {
        backend.cancel_jobs(JobFilters::default()).await.unwrap();
        assert!(all_ready_executions(backend).await.is_empty());

        let definitions = (0..2500).map(|i| job(i % 7 - 3)).collect::<Vec<_>>();
        backend
            .add_jobs(&new_jobs(&definitions), None)
            .await
            .unwrap();

        let ready = all_ready_executions(backend).await;
        assert_eq!(ready.len(), definitions.len());
        assert_ready_order(&ready);

        // Ignored executions are not returned in any of the batches.
        let ignored = ready
            .iter()
            .step_by(3)
            .map(|execution| execution.execution_id)
            .collect::<Vec<_>>();
        let not_ignored = ignored_ready_executions(backend, &ignored).await;
        assert_eq!(not_ignored.len(), ready.len() - ignored.len());
        assert!(
            not_ignored
                .iter()
                .all(|execution| !ignored.contains(&execution.execution_id))
        );
        assert_ready_order(&not_ignored);
    }
}

fn new_jobs(jobs: &[JobDefinition]) -> Vec<NewJob> {
    jobs.iter()
        .map(|j| NewJob {
            job: j.clone(),
            schedule_id: None,
        })
        .collect()
}
