use std::{
    io::Read,
    pin::pin,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Subcommand, ValueEnum};
use clap_complete::ArgValueCompleter;
use comfy_table::{Table, presets};
use eyre::{Context, OptionExt};
use futures::TryStreamExt;
use jiff::{Timestamp, civil::Time, tz::TimeZone};
use ora::{
    AdminClient, JobFilters, JobTypeId,
    common::{LabelFilter, TimeRange},
    execution::{ExecutionId, ExecutionStatus},
    executor::ExecutorId,
    job::JobId,
    proto::jobs::v1::{RetryPolicy, TimeoutBaseTime, TimeoutPolicy},
    schedule::ScheduleId,
};
use serde_json::Value;
use tempfile::NamedTempFile;

use crate::completions::{
    complete_active_job_id, complete_any_job_id, complete_any_schedule_id, complete_job_type,
};

#[derive(Subcommand)]
pub(crate) enum Jobs {
    /// List jobs based on the given criteria.
    #[command(visible_aliases = ["ls"])]
    List {
        #[command(flatten)]
        filters: JobFilterArgs,
        /// The order in which jobs are listed.
        #[arg(long = "order", default_value_t = JobOrder::CreatedDesc)]
        order: JobOrder,
        /// Limit the number of jobs returned.
        #[arg(long = "limit", default_value_t = 100)]
        limit: u32,
    },
    /// Count jobs based on the given criteria.
    Count {
        #[command(flatten)]
        filters: JobFilterArgs,
    },
    /// Add a new job to be executed.
    Add {
        /// The job type ID.
        #[arg(long = "type", add = ArgValueCompleter::new(complete_job_type))]
        job_type_id: String,
        /// The job payload as a JSON string.
        ///
        /// If a value of `-` is provided,
        /// the payload will be read from standard input.
        ///
        /// If the `EDITOR` is set in the environment,
        /// it will be opened in the editor for editing.
        ///
        /// If no value is provided, an empty JSON object will be used.
        payload: Option<String>,
        /// The target execution time as a timestamp or date.
        ///
        /// If not provided, the job will be scheduled for immediate execution.
        ///
        /// If a date is provided, the time is assumed to be 00:00:00 UTC.
        #[arg(long = "target")]
        target_execution_time: Option<String>,
        /// Labels to assign to the job.
        ///
        /// The format is `key=value`.
        ///
        /// Can be specified multiple times,
        /// comma-separated values are also supported.
        #[arg(long = "label", value_delimiter = ',')]
        labels: Vec<String>,
        /// Set how many times the job can be retries.
        #[arg(long, visible_aliases = ["retry"])]
        retries: Option<u64>,
        /// Backoff duration for replies.
        ///
        /// The interval should be a human-readable duration, e.g., `1h`, `30m`.
        #[arg(
            long = "retry-backoff",
            visible_aliases= ["backoff"],
            conflicts_with = "cron_expression"
        )]
        retry_backoff: Option<String>,
        /// The maximum backoff duration for retries.
        ///
        /// The interval should be a human-readable duration, e.g., `1h`, `30m`.
        /// Only applicable if `--retry-backoff-strategy` is set.
        #[arg(
            long = "retry-max-backoff",
            visible_aliases= ["max-backoff"],
            requires = "retry_backoff"
        )]
        retry_max_backoff: Option<String>,
        /// The backoff strategy to use for retries.
        #[arg(
            long = "retry-backoff-strategy",
            visible_aliases = ["backoff-strategy"],
            default_value_t = BackoffStrategy::Fixed
        )]
        retry_backoff_strategy: BackoffStrategy,
        /// Set the timeout of the job in
        /// a human-readable format, e.g., `30s`, `5m`, `1h`.
        #[arg(long)]
        timeout: Option<String>,
        /// Whether to skip any validation when adding the job.
        #[arg(long, short)]
        force: bool,
        /// Whether to wait for the job to complete.
        #[arg(long, short)]
        wait: bool,
    },
    /// Cancel jobs.
    Cancel {
        #[command(flatten)]
        filters: JobFilterArgs,
    },
    /// Wait for a job to complete.
    Wait {
        /// The job ID.
        #[arg(add = ArgValueCompleter::new(complete_active_job_id))]
        job_id: String,
    },
    /// Wait for a job to complete and get its output.
    Output {
        /// The job ID.
        #[arg(add = ArgValueCompleter::new(complete_any_job_id))]
        job_id: String,
    },
    /// Get the input of a job.
    Input {
        /// The job ID.
        #[arg(add = ArgValueCompleter::new(complete_any_job_id))]
        job_id: String,
    },
}

/// Filters for listing jobs.
#[derive(clap::Args)]
pub(crate) struct JobFilterArgs {
    /// Filter for job IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "id", visible_aliases = ["job-id"], value_delimiter = ',')]
    job_ids: Vec<String>,
    /// Filter for job type IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(
            long = "type",
            aliases = ["type-id", "job-type", "job-type-id"],
            value_delimiter = ',',
            add = ArgValueCompleter::new(complete_job_type),
        )]
    job_type_ids: Vec<String>,
    /// Filter for executor IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "executor-id", visible_aliases = ["executor"], value_delimiter = ',')]
    executor_ids: Vec<String>,
    /// Filter for schedule IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "schedule-id", visible_aliases = ["schedule"], value_delimiter = ',')]
    #[arg(add = ArgValueCompleter::new(complete_any_schedule_id))]
    schedule_ids: Vec<String>,
    /// Filter for execution IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "execution-id", visible_aliases = ["execution"], value_delimiter = ',')]
    execution_ids: Vec<String>,
    /// Filter by labels.
    ///
    /// The format can be either `key=value` or just `key`
    /// that will match any value for the given key.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "label", value_delimiter = ',')]
    labels: Vec<String>,
    /// Filter by job status.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "status", value_delimiter = ',')]
    execution_statuses: Vec<JobStatus>,
    /// Filter for jobs with a target execution time
    /// after the given timestamp or date.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "target-after")]
    target_execution_time_after: Option<String>,
    /// Filter for jobs with a target execution time
    /// before the given timestamp or date.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "target-before")]
    target_execution_time_before: Option<String>,
    /// A timestamp or date after which the job was created.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "created-after")]
    created_after: Option<String>,
    /// A timestamp or date before which the job was created.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "created-before")]
    created_before: Option<String>,
}

impl TryFrom<JobFilterArgs> for JobFilters {
    type Error = eyre::Error;

    fn try_from(value: JobFilterArgs) -> Result<Self, Self::Error> {
        let JobFilterArgs {
            job_ids,
            job_type_ids,
            executor_ids,
            schedule_ids,
            execution_ids,
            labels,
            execution_statuses,
            target_execution_time_after,
            target_execution_time_before,
            created_after,
            created_before,
        } = value;

        let target_before = if let Some(ts) = target_execution_time_after {
            Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
        } else {
            None
        };

        let target_after = if let Some(ts) = target_execution_time_before {
            Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
        } else {
            None
        };

        let created_after = if let Some(ts) = created_after {
            Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
        } else {
            None
        };

        let created_before = if let Some(ts) = created_before {
            Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
        } else {
            None
        };

        let statuses = execution_statuses
            .into_iter()
            .map(ExecutionStatus::from)
            .collect::<Vec<_>>();

        Ok(JobFilters {
            job_ids: if job_ids.is_empty() {
                None
            } else {
                Some(
                    job_ids
                        .into_iter()
                        .map(|i| {
                            Result::<_, eyre::Report>::Ok(JobId(
                                i.parse().wrap_err("invalid job ID")?,
                            ))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            job_type_ids: if job_type_ids.is_empty() {
                None
            } else {
                Some(
                    job_type_ids
                        .into_iter()
                        .map(JobTypeId::new)
                        .collect::<Result<_, _>>()?,
                )
            },
            executor_ids: if executor_ids.is_empty() {
                None
            } else {
                Some(
                    executor_ids
                        .into_iter()
                        .map(|i| {
                            Result::<_, eyre::Report>::Ok(ExecutorId(
                                i.parse().wrap_err("invalid job ID")?,
                            ))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            execution_ids: if execution_ids.is_empty() {
                None
            } else {
                Some(
                    execution_ids
                        .into_iter()
                        .map(|i| {
                            Result::<_, eyre::Report>::Ok(ExecutionId(
                                i.parse().wrap_err("invalid execution ID")?,
                            ))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            execution_statuses: if statuses.is_empty() {
                None
            } else {
                Some(statuses)
            },
            target_execution_time: Some(TimeRange {
                start: target_after.map(Into::into),
                end: target_before.map(Into::into),
            }),
            created_at: Some(TimeRange {
                start: created_after.map(Into::into),
                end: created_before.map(Into::into),
            }),
            labels: if labels.is_empty() {
                None
            } else {
                Some(
                    labels
                        .into_iter()
                        .map(|l| {
                            let mut parts = l.trim().splitn(2, '=');
                            let key = parts.next().unwrap_or_default().trim();
                            let value = parts.next();

                            LabelFilter {
                                key: key.to_string(),
                                value: value.map(ToString::to_string),
                            }
                        })
                        .collect(),
                )
            },
            schedule_ids: if schedule_ids.is_empty() {
                None
            } else {
                Some(
                    schedule_ids
                        .into_iter()
                        .map(|i| {
                            Result::<_, eyre::Report>::Ok(ScheduleId(
                                i.parse().wrap_err("invalid schedule ID")?,
                            ))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
        })
    }
}

/// The status of a job.
#[derive(ValueEnum, Clone)]
pub(crate) enum JobStatus {
    /// The job execution is pending.
    Pending,
    /// The job execution is in progress.
    InProgress,
    /// The job execution has completed successfully.
    Succeeded,
    /// The job execution has failed.
    Failed,
    /// The job execution was cancelled.
    Cancelled,
}

impl core::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            JobStatus::Pending => "pending",
            JobStatus::InProgress => "in-progress",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}

impl From<JobStatus> for ExecutionStatus {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => ExecutionStatus::Pending,
            JobStatus::InProgress => ExecutionStatus::InProgress,
            JobStatus::Succeeded => ExecutionStatus::Succeeded,
            JobStatus::Failed => ExecutionStatus::Failed,
            JobStatus::Cancelled => ExecutionStatus::Cancelled,
        }
    }
}

impl From<ExecutionStatus> for JobStatus {
    fn from(status: ExecutionStatus) -> Self {
        match status {
            ExecutionStatus::Pending => JobStatus::Pending,
            ExecutionStatus::InProgress => JobStatus::InProgress,
            ExecutionStatus::Succeeded => JobStatus::Succeeded,
            ExecutionStatus::Failed => JobStatus::Failed,
            ExecutionStatus::Cancelled => JobStatus::Cancelled,
        }
    }
}

/// The backoff strategy for retries.
#[derive(ValueEnum, Clone)]
pub(crate) enum BackoffStrategy {
    /// Exponential backoff strategy.
    Exponential,
    /// Fixed backoff strategy.
    Fixed,
}

impl core::fmt::Display for BackoffStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            BackoffStrategy::Exponential => "exponential",
            BackoffStrategy::Fixed => "fixed",
        };
        write!(f, "{s}")
    }
}

impl From<BackoffStrategy> for ora::proto::jobs::v1::BackoffStrategy {
    fn from(strategy: BackoffStrategy) -> Self {
        match strategy {
            BackoffStrategy::Exponential => ora::proto::jobs::v1::BackoffStrategy::Exponential as _,
            BackoffStrategy::Fixed => ora::proto::jobs::v1::BackoffStrategy::Fixed as _,
        }
    }
}

impl From<ora::proto::jobs::v1::BackoffStrategy> for BackoffStrategy {
    fn from(strategy: ora::proto::jobs::v1::BackoffStrategy) -> Self {
        match strategy {
            ora::proto::jobs::v1::BackoffStrategy::Exponential => BackoffStrategy::Exponential,
            _ => BackoffStrategy::Fixed, // Default to Fixed for unrecognized values
        }
    }
}

/// The order in which jobs are listed.
#[derive(ValueEnum, Clone)]
pub(crate) enum JobOrder {
    /// Order by creation time ascending.
    CreatedAsc,
    /// Order by creation time descending.
    CreatedDesc,
    /// Order by target execution time ascending.
    TargetAsc,
    /// Order by target execution time descending.
    TargetDesc,
}

impl core::fmt::Display for JobOrder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            JobOrder::CreatedAsc => "created-asc",
            JobOrder::CreatedDesc => "created-desc",
            JobOrder::TargetAsc => "target-asc",
            JobOrder::TargetDesc => "target-desc",
        };
        write!(f, "{s}")
    }
}

impl From<JobOrder> for ora::JobOrderBy {
    fn from(order: JobOrder) -> Self {
        match order {
            JobOrder::CreatedAsc => ora::JobOrderBy::CreatedAtAsc,
            JobOrder::CreatedDesc => ora::JobOrderBy::CreatedAtDesc,
            JobOrder::TargetAsc => ora::JobOrderBy::TargetExecutionTimeAsc,
            JobOrder::TargetDesc => ora::JobOrderBy::TargetExecutionTimeDesc,
        }
    }
}

impl Jobs {
    pub(crate) async fn execute(self, client: AdminClient) -> eyre::Result<()> {
        match self {
            Jobs::List {
                filters,
                order,
                limit,
            } => {
                list_jobs(&client, order, limit, filters.try_into()?).await?;

                Ok(())
            }
            Jobs::Count { filters } => {
                let filters = filters.try_into()?;
                let count = client.count_jobs(filters).await?;
                println!("{count}");
                Ok(())
            }
            Jobs::Add {
                job_type_id,
                payload,
                target_execution_time,
                labels,
                force,
                retries,
                timeout,
                wait,
                retry_backoff,
                retry_backoff_strategy,
                retry_max_backoff,
            } => {
                let job_type_id = JobTypeId::new(job_type_id).wrap_err("invalid job type ID")?;

                let mut job_types = None;

                let mut provided_via_editor = false;
                let mut payload = match payload {
                    Some(p) => {
                        if p == "-" {
                            tracing::info!("reading payload from stdin");
                            let mut buf = String::new();
                            std::io::stdin().read_to_string(&mut buf)?;
                            buf
                        } else {
                            p
                        }
                    }
                    None => {
                        if let Ok(editor) = std::env::var("EDITOR") {
                            let tmpfile = tempfile::NamedTempFile::with_suffix(".json")
                                .wrap_err("failed to create temporary file")?;

                            // Try to get the schema of the job type for better intellisense.
                            job_types = match client.list_job_types().await {
                                Ok(jts) => Some(jts),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to fetch job types for schema retrieval: {e}"
                                    );
                                    None
                                }
                            };

                            let mut default_editor_content = "{}".to_string();
                            let json_temp = NamedTempFile::with_suffix(".json")
                                .wrap_err("failed to create temporary file")?;

                            if let Some(jt) = job_types.as_ref().and_then(|jts| {
                                jts.iter().find(|jt| jt.id.as_str() == job_type_id.as_str())
                            }) && let Some(schema) = &jt.input_schema_json
                            {
                                std::fs::write(json_temp.path(), schema)?;
                                default_editor_content = format!(
                                    r#"{{
  "$schema": "file://{}"
}}"#,
                                    json_temp.path().to_string_lossy()
                                );
                            }

                            std::fs::write(tmpfile.path(), default_editor_content)
                                .wrap_err("failed to write to temporary file")?;

                            let mut editor_args = editor.split_whitespace();

                            let mut cmd = std::process::Command::new(
                                editor_args.next().ok_or_eyre("failed to open editor")?,
                            );
                            cmd.args(editor_args);
                            cmd.arg(tmpfile.path());

                            let status = cmd.status().wrap_err("failed to open editor")?;

                            if !status.success() {
                                return Err(eyre::eyre!("editor exited with non-zero status"));
                            }

                            provided_via_editor = true;

                            std::fs::read_to_string(tmpfile.path())
                                .wrap_err("failed to read temporary file")?
                        } else {
                            "{}".into()
                        }
                    }
                };

                let mut payload_value: Value =
                    serde_json::from_str(&payload).wrap_err("payload must be valid JSON")?;

                if provided_via_editor && let Some(p) = payload_value.as_object_mut() {
                    p.remove("$schema");
                    payload = serde_json::to_string_pretty(&payload_value)
                        .wrap_err("failed to serialize payload")?;
                }

                let validate = !force;

                if validate {
                    // Make sure the job type exists and its schema is valid.
                    let job_types = match job_types {
                        Some(jts) => jts,
                        None => client
                            .list_job_types()
                            .await
                            .wrap_err("failed to fetch job types")?,
                    };

                    let job_type = job_types
                        .into_iter()
                        .find(|jt| jt.id.as_str() == job_type_id.as_str())
                        .ok_or_eyre("job type does not exist, use --force to add the job without validation")?;

                    if let Some(schema) = job_type.input_schema_json
                        && !jsonschema::is_valid(
                            &serde_json::from_str(&schema)
                                .wrap_err("invalid JSON schema, use --force to skip validation")?,
                            &payload_value,
                        )
                    {
                        return Err(eyre::eyre!(
                            "payload does not conform to the job type's input schema, use --force to skip validation"
                        ));
                    }
                }

                let target_execution_time = if let Some(ts) = target_execution_time {
                    parse_ts_or_date(&ts).wrap_err("unrecognized time format")?
                } else {
                    Timestamp::now()
                };

                let retry_backoff = if let Some(backoff) = retry_backoff {
                    Some(
                        humantime::parse_duration(&backoff)
                            .wrap_err("invalid retry backoff duration")?,
                    )
                } else {
                    None
                };

                let retry_max_backoff = if let Some(max_backoff) = retry_max_backoff {
                    Some(
                        humantime::parse_duration(&max_backoff)
                            .wrap_err("invalid retry max backoff duration")?,
                    )
                } else {
                    None
                };

                let mut job = client
                    .add_jobs([ora::proto::jobs::v1::Job {
                        job_type_id: job_type_id.clone().into_inner().into(),
                        target_execution_time: Some(SystemTime::from(target_execution_time).into()),
                        input_payload_json: payload,
                        labels: labels
                            .into_iter()
                            .filter_map(|l| {
                                let mut parts = l.trim().splitn(2, '=');
                                let key = parts.next()?.trim();
                                let value = parts.next()?.trim();
                                if key.is_empty() || value.is_empty() {
                                    None
                                } else {
                                    Some(ora::proto::common::v1::Label {
                                        key: key.to_string(),
                                        value: value.to_string(),
                                    })
                                }
                            })
                            .collect(),
                        timeout_policy: Some(TimeoutPolicy {
                            timeout: timeout
                                .map(|t| humantime::parse_duration(&t))
                                .transpose()
                                .wrap_err("invalid timeout duration")?
                                .and_then(|d| d.try_into().ok()),
                            base_time: TimeoutBaseTime::StartTime as _,
                        }),
                        retry_policy: Some(RetryPolicy {
                            retries: retries.unwrap_or(0),
                            backoff_strategy: ora::proto::jobs::v1::BackoffStrategy::from(
                                retry_backoff_strategy,
                            ) as _,
                            backoff_duration: retry_backoff.map(TryInto::try_into).transpose()?,
                            max_backoff_duration: retry_max_backoff
                                .map(TryInto::try_into)
                                .transpose()?,
                        }),
                    }])
                    .await?
                    .pop()
                    .ok_or_eyre("failed to add job")?;

                if wait {
                    tracing::info!("waiting for job...");
                    loop {
                        job.executions_changed().await?;

                        list_jobs(
                            &client,
                            JobOrder::CreatedDesc,
                            1,
                            JobFilters {
                                job_ids: Some(vec![job.id()]),
                                ..Default::default()
                            },
                        )
                        .await?;

                        if job.is_terminated().await? {
                            break;
                        }
                    }

                    let last_exec = job.executions().await?.pop().unwrap();

                    if let Some(output) = last_exec.output_json() {
                        println!("{output}");
                    } else if let Some(reason) = last_exec.failure_reason() {
                        println!("{reason}");
                        std::process::exit(1);
                    } else {
                        std::process::exit(1);
                    }
                } else {
                    list_jobs(
                        &client,
                        JobOrder::CreatedDesc,
                        1,
                        JobFilters {
                            job_ids: Some(vec![job.id()]),
                            ..Default::default()
                        },
                    )
                    .await?;
                }

                Ok(())
            }
            Jobs::Wait { job_id, .. } => {
                let job_id = JobId(job_id.parse().wrap_err("invalid job ID")?);

                let mut stream = pin!(client.list_jobs(
                    JobFilters {
                        job_ids: Some(vec![job_id]),
                        ..Default::default()
                    },
                    ora::JobOrderBy::CreatedAtAsc,
                    Some(1),
                ));

                let mut job = stream.try_next().await?.ok_or_eyre("job not found")?;

                tracing::info!("waiting for job...");
                loop {
                    job.executions_changed().await?;

                    list_jobs(
                        &client,
                        JobOrder::CreatedDesc,
                        1,
                        JobFilters {
                            job_ids: Some(vec![job.id()]),
                            ..Default::default()
                        },
                    )
                    .await?;

                    if job.is_terminated().await? {
                        break;
                    }
                }

                let last_exec = job.executions().await?.pop().unwrap();

                if let Some(output) = last_exec.output_json() {
                    println!("{output}");
                } else if let Some(reason) = last_exec.failure_reason() {
                    println!("{reason}");
                    std::process::exit(1);
                } else {
                    std::process::exit(1);
                }

                Ok(())
            }
            Jobs::Output { job_id, .. } => {
                let job_id = JobId(job_id.parse().wrap_err("invalid job ID")?);

                let mut stream = pin!(client.list_jobs(
                    JobFilters {
                        job_ids: Some(vec![job_id]),
                        ..Default::default()
                    },
                    ora::JobOrderBy::CreatedAtAsc,
                    Some(1),
                ));

                let mut job = stream.try_next().await?.ok_or_eyre("job not found")?;

                job.terminated().await?;

                let last_exec = job.executions().await?.pop().unwrap();

                if let Some(output) = last_exec.output_json() {
                    println!("{output}");
                } else if let Some(reason) = last_exec.failure_reason() {
                    println!("{reason}");
                    std::process::exit(1);
                } else {
                    std::process::exit(1);
                }

                Ok(())
            }
            Jobs::Cancel { filters } => {
                let job_filters: JobFilters = filters.try_into()?;

                let jobs = client.cancel_jobs(job_filters).await?;

                tracing::info!("cancelled jobs");

                let job_ids = jobs.into_iter().map(|j| j.id()).collect::<Vec<_>>();

                list_jobs(
                    &client,
                    JobOrder::CreatedDesc,
                    job_ids.len().try_into().unwrap_or(0),
                    JobFilters {
                        job_ids: Some(job_ids),
                        ..Default::default()
                    },
                )
                .await?;

                Ok(())
            }
            Jobs::Input { job_id } => {
                let job_id = JobId(job_id.parse().wrap_err("invalid job ID")?);

                let mut stream = pin!(client.list_jobs(
                    JobFilters {
                        job_ids: Some(vec![job_id]),
                        ..Default::default()
                    },
                    ora::JobOrderBy::CreatedAtAsc,
                    Some(1),
                ));

                let mut job = stream.try_next().await?.ok_or_eyre("job not found")?;

                println!(
                    "{}",
                    job.raw()
                        .await?
                        .job
                        .ok_or_eyre("job data is missing")?
                        .input_payload_json
                );

                Ok(())
            }
        }
    }
}

async fn list_jobs(
    client: &AdminClient,
    order: JobOrder,
    limit: u32,
    filters: JobFilters,
) -> Result<(), eyre::Error> {
    use core::fmt::Write;

    let mut stream = pin!(client.list_jobs(filters, order.into(), Some(limit)));
    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL);
    table.set_header(["Type", "Target", "Status", "Labels", "Retries", "Misc"]);
    while let Some(mut job) = stream.try_next().await? {
        let last_exec = job.executions().await?.pop().unwrap();
        let raw = job.raw().await?;
        let raw_def = raw.job.ok_or_eyre("missing job data")?;

        let mut labels = String::new();
        for label in raw_def.labels {
            if !labels.is_empty() {
                labels.push('\n');
            }
            write!(&mut labels, "{}={}", label.key, label.value).unwrap();
        }

        let mut status = JobStatus::from(last_exec.status()).to_string();

        if let (Some(started), Some(ended)) = (raw_def.target_execution_time, last_exec.ended_at())
        {
            let dur = ended
                .duration_since(SystemTime::try_from(started).unwrap_or(UNIX_EPOCH))
                .unwrap_or_default();
            write!(&mut status, " ({})", humantime::format_duration(dur)).unwrap();
        }

        let raw_retry_policy = raw_def.retry_policy.unwrap_or_default();

        let job_attempt_count = raw.executions.len().saturating_sub(1) as u64;

        let mut retries = String::new();

        if raw_retry_policy.retries > 0 {
            writeln!(
                &mut retries,
                "{}/{}\n",
                job_attempt_count, raw_retry_policy.retries
            )
            .unwrap();

            writeln!(
                &mut retries,
                "backoff: {}",
                raw_retry_policy
                    .backoff_duration
                    .map(
                        |d| humantime::format_duration(d.try_into().unwrap_or_default())
                            .to_string()
                    )
                    .unwrap_or_else(|| "none".to_string())
            )
            .unwrap();

            write!(
                &mut retries,
                "{}",
                BackoffStrategy::from(raw_retry_policy.backoff_strategy())
            )
            .unwrap();
        } else {
            retries.push('-');
        }

        let mut misc = String::new();
        misc.push_str("ID:\n");
        writeln!(&mut misc, " {}", job.id()).unwrap();

        misc.push_str("schedule ID:\n");
        if let Some(schedule_id) = raw.schedule_id {
            writeln!(&mut misc, " {schedule_id}").unwrap();
        } else {
            misc.push_str(" -");
        }

        table.add_row([
            raw_def.job_type_id,
            Timestamp::try_from(SystemTime::try_from(
                raw_def.target_execution_time.ok_or_eyre("missing")?,
            )?)?
            .to_zoned(TimeZone::system())
            .to_string(),
            status,
            labels,
            retries,
            misc,
        ]);
    }
    println!("{table}");
    Ok(())
}

fn parse_ts_or_date(ts: &str) -> eyre::Result<Timestamp> {
    if let Ok(ts) = ts.parse::<Timestamp>() {
        return Ok(ts);
    }

    Ok(jiff::fmt::strtime::parse("%Y-%m-%d", ts)?
        .to_date()?
        .to_datetime(Time::midnight())
        .in_tz("UTC")?
        .timestamp())
}
