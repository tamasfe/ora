use std::{io::Read, pin::pin, time::SystemTime};

use clap::{Subcommand, ValueEnum};
use clap_complete::ArgValueCompleter;
use comfy_table::{Table, presets};
use eyre::{Context, OptionExt, bail};
use futures::TryStreamExt;
use jiff::{Timestamp, civil::Time};
use ora::{
    AdminClient, JobTypeId, ScheduleFilters,
    common::{LabelFilter, TimeRange},
    proto::jobs::v1::{RetryPolicy, TimeoutBaseTime, TimeoutPolicy},
    schedule::{ScheduleId, SchedulingPolicy},
};
use serde_json::Value;
use tempfile::NamedTempFile;

use crate::completions::{complete_active_schedule_id, complete_job_type};

#[derive(Subcommand)]
pub(crate) enum Schedules {
    /// List schedules based on the given criteria.
    #[command(visible_aliases = ["ls"])]
    List {
        #[command(flatten)]
        filters: ScheduleFilterArgs,
        /// The order in which schedules are listed.
        #[arg(long = "order", default_value_t = ScheduleOrder::CreatedDesc)]
        order: ScheduleOrder,
        /// Limit the number of schedules returned.
        #[arg(long = "limit", default_value_t = 100)]
        limit: u32,
    },
    /// Count schedules based on the given criteria.
    Count {
        #[command(flatten)]
        filters: ScheduleFilterArgs,
    },
    /// Add a new schedule.
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
        /// Labels to assign to the schedule and its jobs.
        ///
        /// The format is `key=value`.
        ///
        /// Can be specified multiple times,
        /// comma-separated values are also supported.
        #[arg(long = "label", value_delimiter = ',')]
        labels: Vec<String>,
        /// Set how many times the schedule can be retries.
        #[arg(long, visible_aliases = ["retry"])]
        retries: Option<u64>,
        /// Set the timeout of the jobs in
        /// a human-readable format, e.g., `30s`, `5m`, `1h`.
        #[arg(long)]
        timeout: Option<String>,
        /// Repeat jobs with a given interval.
        ///
        /// The interval should be a human-readable duration, e.g., `1h`, `30m`.
        #[arg(long = "repeat", conflicts_with = "cron")]
        repeat_interval: Option<String>,
        /// Repeat jobs according to a cron expression.
        ///
        /// The expression should be in standard cron format with
        /// an optional timezone suffix, e.g., `0 0 * * * UTC`.
        #[arg(long = "cron", conflicts_with = "repeat")]
        cron_expression: Option<String>,
        /// Whether to spawn a job immediately upon adding the schedule.
        #[arg(long)]
        immediate: bool,
        /// Only start the schedule after the given timestamp or date.
        ///
        /// If a date is provided, the time is assumed to be 00:00:00 UTC.
        #[arg(long = "target-after")]
        start_after: Option<String>,
        /// Stop the schedule before the given timestamp or date.
        ///
        /// If a date is provided, the time is assumed to be 00:00:00 UTC.
        #[arg(long = "target-before")]
        stop_before: Option<String>,
        /// The action to take when a job time is missed.
        #[arg(long, default_value_t = MissedTimePolicyArg::Skip)]
        missed_job_action: MissedTimePolicyArg,
        /// Whether to skip any validation when adding the schedule.
        #[arg(long, short)]
        force: bool,
    },
    /// Stop schedules.
    Stop {
        /// Filter by schedule IDs.
        ///
        /// Can be specified multiple times,
        /// comma-separated values are also supported.
        #[arg(long = "id", value_delimiter = ',')]
        #[arg(add = ArgValueCompleter::new(complete_active_schedule_id))]
        schedule_ids: Vec<String>,

        /// Whether to ignore the jobs
        /// of the stopped schedules.
        ///
        /// By default jobs are cancelled
        /// when stopping schedules.
        #[arg(long)]
        ignore_jobs: bool,

        #[command(flatten)]
        filters: ScheduleFilterArgs,
    },
}

/// Filters for listing schedules.
#[derive(clap::Args)]
pub(crate) struct ScheduleFilterArgs {
    /// Filter for schedule IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "id", visible_aliases = ["schedule-id"], value_delimiter = ',')]
    schedule_ids: Vec<String>,
    /// Filter for schedule type IDs.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(
            long = "type",
            aliases = ["type-id", "schedule-type", "schedule-type-id"],
            value_delimiter = ',',
            add = ArgValueCompleter::new(complete_job_type),
        )]
    job_type_ids: Vec<String>,
    /// Filter by labels.
    ///
    /// The format can be either `key=value` or just `key`
    /// that will match any value for the given key.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "label", value_delimiter = ',')]
    labels: Vec<String>,
    /// Filter by schedule status.
    ///
    /// Can be specified multiple times,
    /// comma-separated values are also supported.
    #[arg(long = "status", value_delimiter = ',')]
    statuses: Vec<ScheduleStatusArg>,
    /// A timestamp or date after which the schedule was created.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "created-after")]
    created_after: Option<String>,
    /// A timestamp or date before which the schedule was created.
    ///
    /// If a date is provided, the time is assumed to be 00:00:00 UTC.
    #[arg(long = "created-before")]
    created_before: Option<String>,
}

impl TryFrom<ScheduleFilterArgs> for ScheduleFilters {
    type Error = eyre::Error;

    fn try_from(value: ScheduleFilterArgs) -> Result<Self, Self::Error> {
        let ScheduleFilterArgs {
            schedule_ids,
            job_type_ids,
            labels,
            statuses,
            created_after,
            created_before,
        } = value;

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

        let statuses = statuses
            .into_iter()
            .map(ora::ScheduleStatus::from)
            .collect::<Vec<_>>();

        Ok(ScheduleFilters {
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
            statuses: if statuses.is_empty() {
                None
            } else {
                Some(statuses)
            },
        })
    }
}

/// Policy for handling missed execution times.
#[derive(ValueEnum, Clone)]
pub enum MissedTimePolicyArg {
    /// Skip missed execution times.
    Skip,
    /// Create jobs for missed execution times.
    Create,
}

impl core::fmt::Display for MissedTimePolicyArg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            MissedTimePolicyArg::Skip => "skip",
            MissedTimePolicyArg::Create => "create",
        };
        write!(f, "{s}")
    }
}

impl From<MissedTimePolicyArg> for ora::schedule::MissedTimePolicy {
    fn from(policy: MissedTimePolicyArg) -> Self {
        match policy {
            MissedTimePolicyArg::Skip => ora::schedule::MissedTimePolicy::Skip,
            MissedTimePolicyArg::Create => ora::schedule::MissedTimePolicy::Create,
        }
    }
}

/// The status of a schedule.
#[derive(ValueEnum, Clone)]
pub(crate) enum ScheduleStatusArg {
    /// The schedule is active.
    Active,
    /// The schedule is stopped.
    Stopped,
}

impl core::fmt::Display for ScheduleStatusArg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            ScheduleStatusArg::Active => "active",
            ScheduleStatusArg::Stopped => "stopped",
        };
        write!(f, "{s}")
    }
}

impl From<ScheduleStatusArg> for ora::ScheduleStatus {
    fn from(status: ScheduleStatusArg) -> Self {
        match status {
            ScheduleStatusArg::Active => ora::ScheduleStatus::Active,
            ScheduleStatusArg::Stopped => ora::ScheduleStatus::Stopped,
        }
    }
}

impl From<ora::ScheduleStatus> for ScheduleStatusArg {
    fn from(status: ora::ScheduleStatus) -> Self {
        match status {
            ora::ScheduleStatus::Active => ScheduleStatusArg::Active,
            ora::ScheduleStatus::Stopped => ScheduleStatusArg::Stopped,
        }
    }
}

/// The order in which schedules are listed.
#[derive(ValueEnum, Clone)]
pub(crate) enum ScheduleOrder {
    /// Order by creation time ascending.
    CreatedAsc,
    /// Order by creation time descending.
    CreatedDesc,
}

impl core::fmt::Display for ScheduleOrder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            ScheduleOrder::CreatedAsc => "created-asc",
            ScheduleOrder::CreatedDesc => "created-desc",
        };
        write!(f, "{s}")
    }
}

impl From<ScheduleOrder> for ora::ScheduleOrderBy {
    fn from(order: ScheduleOrder) -> Self {
        match order {
            ScheduleOrder::CreatedAsc => ora::ScheduleOrderBy::CreatedAtAsc,
            ScheduleOrder::CreatedDesc => ora::ScheduleOrderBy::CreatedAtDesc,
        }
    }
}

impl Schedules {
    pub(crate) async fn execute(self, client: AdminClient) -> eyre::Result<()> {
        match self {
            Schedules::List {
                filters,
                order,
                limit,
            } => {
                list_schedules(&client, order, limit, filters.try_into()?).await?;

                Ok(())
            }
            Schedules::Count { filters } => {
                let count = client.count_schedules(filters.try_into()?).await?;
                println!("{count}");
                Ok(())
            }
            Schedules::Add {
                job_type_id,
                payload,
                labels,
                force,
                retries,
                timeout,
                repeat_interval,
                cron_expression,
                immediate,
                start_after,
                stop_before,
                missed_job_action,
            } => {
                let job_type_id =
                    JobTypeId::new(job_type_id).wrap_err("invalid schedule type ID")?;

                let start_after = if let Some(ts) = start_after {
                    Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
                } else {
                    None
                };

                let stop_before = if let Some(ts) = stop_before {
                    Some(parse_ts_or_date(&ts).wrap_err("unrecognized time format")?)
                } else {
                    None
                };

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

                            // Try to get the schema of the schedule type for better intellisense.
                            job_types = match client.list_job_types().await {
                                Ok(jts) => Some(jts),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to fetch schedule types for schema retrieval: {e}"
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
                    // Make sure the schedule type exists and its schema is valid.
                    let job_types = match job_types {
                        Some(jts) => jts,
                        None => client
                            .list_job_types()
                            .await
                            .wrap_err("failed to fetch schedule types")?,
                    };

                    let schedule_type = job_types
                        .into_iter()
                        .find(|jt| jt.id.as_str() == job_type_id.as_str())
                        .ok_or_eyre("schedule type does not exist, use --force to add the schedule without validation")?;

                    if let Some(schema) = schedule_type.input_schema_json
                        && !jsonschema::is_valid(
                            &serde_json::from_str(&schema)
                                .wrap_err("invalid JSON schema, use --force to skip validation")?,
                            &payload_value,
                        )
                    {
                        return Err(eyre::eyre!(
                            "payload does not conform to the schedule type's input schema, use --force to skip validation"
                        ));
                    }
                }

                let schedule = client
                    .add_schedules([ora::proto::schedules::v1::Schedule {
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
                        job_template: Some(ora::proto::jobs::v1::Job {
                            job_type_id: job_type_id.clone().into_inner().into(),
                            target_execution_time: Some(SystemTime::UNIX_EPOCH.into()),
                            input_payload_json: payload,
                            labels: vec![],
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
                            }),
                        }),
                        scheduling: if let Some(interval) = repeat_interval {
                            Some(
                                SchedulingPolicy::FixedInterval {
                                    interval: humantime::parse_duration(&interval)
                                        .wrap_err("invalid repeat interval")?,
                                    immediate,
                                    missed: missed_job_action.into(),
                                }
                                .into(),
                            )
                        } else if let Some(cron) = cron_expression {
                            let mut opts = cronexpr::ParseOptions::default();
                            opts.fallback_timezone_option = cronexpr::FallbackTimezoneOption::UTC;

                            _ = cronexpr::parse_crontab_with(&cron, opts)
                                .wrap_err("invalid cron expression")?;

                            Some(
                                SchedulingPolicy::Cron {
                                    expression: cron,
                                    immediate,
                                    missed: missed_job_action.into(),
                                }
                                .into(),
                            )
                        } else {
                            bail!("either --repeat or --cron must be specified")
                        },
                        time_range: Some(
                            TimeRange {
                                start: start_after.map(Into::into),
                                end: stop_before.map(Into::into),
                            }
                            .into(),
                        ),
                    }])
                    .await?
                    .pop()
                    .ok_or_eyre("failed to add schedule")?;

                list_schedules(
                    &client,
                    ScheduleOrder::CreatedDesc,
                    1,
                    ScheduleFilters {
                        schedule_ids: Some(vec![schedule.id()]),
                        ..Default::default()
                    },
                )
                .await?;

                Ok(())
            }
            Schedules::Stop {
                filters,
                schedule_ids,
                ignore_jobs,
            } => {
                let schedule_ids = if schedule_ids.is_empty() {
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
                };

                let mut schedule_filters: ScheduleFilters = filters.try_into()?;
                schedule_filters.schedule_ids = schedule_ids;

                let schedules = client
                    .stop_schedules(
                        schedule_filters,
                        if ignore_jobs {
                            ora::admin::schedules::StoppedScheduleJobAction::Ignore
                        } else {
                            ora::admin::schedules::StoppedScheduleJobAction::Cancel
                        },
                    )
                    .await?;

                tracing::info!("stopped schedules");

                let schedule_ids = schedules.into_iter().map(|j| j.id()).collect::<Vec<_>>();

                list_schedules(
                    &client,
                    ScheduleOrder::CreatedDesc,
                    schedule_ids.len().try_into().unwrap_or(0),
                    ScheduleFilters {
                        schedule_ids: Some(schedule_ids),
                        ..Default::default()
                    },
                )
                .await?;

                Ok(())
            }
        }
    }
}

async fn list_schedules(
    client: &AdminClient,
    order: ScheduleOrder,
    limit: u32,
    filters: ScheduleFilters,
) -> Result<(), eyre::Error> {
    use core::fmt::Write;

    let mut stream = pin!(client.list_schedules(filters, order.into(), Some(limit)));
    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL);
    table.set_style(comfy_table::TableComponent::HeaderLines, '=');
    table.set_header(["Type", "Status", "Policy", "Labels", "ID"]);
    while let Some(mut schedule) = stream.try_next().await? {
        let raw = schedule.raw().await?;
        let raw_def = raw.schedule.ok_or_eyre("missing schedule data")?;
        let raw_job_def = raw_def.job_template.ok_or_eyre("missing job template")?;

        let mut labels = String::new();
        for label in raw_def.labels {
            if !labels.is_empty() {
                labels.push('\n');
            }
            write!(&mut labels, "{}={}", label.key, label.value).unwrap();
        }

        let status = ScheduleStatusArg::from(schedule.status().await?).to_string();

        let policy = SchedulingPolicy::try_from(
            raw_def.scheduling.ok_or_eyre("missing scheduling policy")?,
        )?;

        let policy = match policy {
            SchedulingPolicy::FixedInterval { interval, .. } => {
                format!("every {}", humantime::format_duration(interval),)
            }
            SchedulingPolicy::Cron { expression, .. } => expression,
        };

        table.add_row([
            raw_job_def.job_type_id,
            status,
            policy,
            labels,
            schedule.id().to_string(),
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
