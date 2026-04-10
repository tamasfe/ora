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
        ExecutionStatus, FailedExecution, RetriedExecution, StartedExecution, SucceededExecution,
    },
    jobs::{JobDefinition, JobFilters, JobTypeId, NewJob, RetryPolicy, TimeoutPolicy},
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

    let ready_executions = pin!(backend.ready_executions())
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready_executions.len(), job_definitions.len());
    assert!(
        ready_executions
            .windows(2)
            .all(|w| w[0].execution_id <= w[1].execution_id)
    );

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

    let ready_executions_2 = pin!(backend.ready_executions()).try_next().await.unwrap();
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

    let ready_executions = pin!(backend.ready_executions())
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

    let ready_executions = pin!(backend.ready_executions()).try_next().await.unwrap();
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
    }]
    .to_vec();
    let result = backend
        .add_jobs(&new_jobs(&job_definitions), None)
        .await
        .expect("Failed to add jobs");
    assert_eq!(result.job_ids().len(), job_definitions.len());

    let ready_executions = pin!(backend.ready_executions())
        .try_next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready_executions.len(), job_definitions.len());

    let cancelled_jobs = backend.cancel_jobs(JobFilters::default()).await.unwrap();

    assert_eq!(cancelled_jobs.len(), job_definitions.len());

    let ready_executions = pin!(backend.ready_executions()).try_next().await.unwrap();
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

fn new_jobs(jobs: &[JobDefinition]) -> Vec<NewJob> {
    jobs.iter()
        .map(|j| NewJob {
            job: j.clone(),
            schedule_id: None,
        })
        .collect()
}
