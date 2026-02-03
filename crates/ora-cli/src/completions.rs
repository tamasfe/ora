use std::{ffi::OsStr, fmt::Write, time::Duration};

use clap::Parser;
use clap_complete::CompletionCandidate;
use futures::TryStreamExt;
use ora::{
    JobFilters, JobTypeId, ScheduleFilters, ScheduleStatus, execution::ExecutionStatus,
    proto::admin::v1::admin_service_client::AdminServiceClient,
};
use tokio::time::timeout;
use tonic::transport::Endpoint;

use crate::commands::{jobs::JobStatus, schedules::ScheduleStatusArg};

pub(crate) fn complete_active_job_id(_: &OsStr) -> Vec<CompletionCandidate> {
    job_id_completions(true)
}

pub(crate) fn complete_any_job_id(_: &OsStr) -> Vec<CompletionCandidate> {
    job_id_completions(false)
}

fn job_id_completions(active_only: bool) -> Vec<CompletionCandidate> {
    #[derive(Parser)]
    #[command(ignore_errors(true))]
    struct OraArgs {
        #[arg(long = "type", global = true)]
        job_type_id: Option<String>,
    }

    let Some(ora_url) = get_ora_url() else {
        return vec![];
    };

    let job_type_id = OraArgs::try_parse_from(std::env::args().skip(2))
        .ok()
        .and_then(|args| JobTypeId::new(args.job_type_id?).ok());

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let client = ora::AdminClient::new(AdminServiceClient::new(
                Endpoint::from_shared(ora_url).unwrap().connect_lazy(),
            ));

            let mut filters = JobFilters::all();

            if active_only {
                filters = filters.active_only();
            }

            if let Some(job_type_id) = job_type_id {
                filters.job_type_ids = Some(vec![job_type_id]);
            }

            let mut jobs = timeout(
                Duration::from_secs(10),
                client
                    .list_jobs(filters, ora::JobOrderBy::CreatedAtDesc, Some(50))
                    .map_ok(|j| {
                        (
                            j.id(),
                            j.raw_cached()
                                .unwrap()
                                .job
                                .as_ref()
                                .unwrap()
                                .job_type_id
                                .clone(),
                            JobStatus::from(ExecutionStatus::from(
                                j.raw_cached().unwrap().executions.last().unwrap().status(),
                            )),
                            j.raw_cached()
                                .unwrap()
                                .job
                                .as_ref()
                                .unwrap()
                                .labels
                                .iter()
                                .fold(String::new(), |mut acc, label| {
                                    if !acc.is_empty() {
                                        acc.push(',');
                                    }

                                    write!(&mut acc, "{}={}", label.key, label.value).unwrap();
                                    acc
                                }),
                        )
                    })
                    .try_collect::<Vec<_>>(),
            )
            .await
            .unwrap()
            .unwrap();

            jobs.sort_by(|(a, ..), (b, ..)| a.cmp(b));

            let mut candidates = Vec::with_capacity(jobs.len());

            for (i, (job_id, job_type, status, mut labels)) in jobs.into_iter().enumerate() {
                if !labels.is_empty() {
                    labels = format!(" [{labels}]");
                }

                candidates.push(
                    CompletionCandidate::new(job_id.to_string())
                        .help(Some(format!("{job_type} ({status}){labels}").into()))
                        .display_order(Some(i)),
                );
            }

            candidates
        })
}

pub(crate) fn complete_active_schedule_id(_: &OsStr) -> Vec<CompletionCandidate> {
    schedule_id_completions(true)
}

pub(crate) fn complete_any_schedule_id(_: &OsStr) -> Vec<CompletionCandidate> {
    schedule_id_completions(false)
}

fn schedule_id_completions(active_only: bool) -> Vec<CompletionCandidate> {
    #[derive(Parser)]
    #[command(ignore_errors(true))]
    struct OraArgs {
        #[arg(long = "type", global = true)]
        job_type_id: Option<String>,
    }

    let Some(ora_url) = get_ora_url() else {
        return vec![];
    };

    let job_type_id = OraArgs::try_parse_from(std::env::args().skip(2))
        .ok()
        .and_then(|args| JobTypeId::new(args.job_type_id?).ok());

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let client = ora::AdminClient::new(AdminServiceClient::new(
                Endpoint::from_shared(ora_url).unwrap().connect_lazy(),
            ));

            let mut filters = ScheduleFilters::all();

            if active_only {
                filters = filters.active_only();
            }

            if let Some(job_type_id) = job_type_id {
                filters.job_type_ids = Some(vec![job_type_id]);
            }

            let mut jobs = timeout(
                Duration::from_secs(10),
                client
                    .list_schedules(filters, ora::ScheduleOrderBy::CreatedAtDesc, Some(50))
                    .map_ok(|s| {
                        (
                            s.id(),
                            s.raw_cached()
                                .unwrap()
                                .schedule
                                .as_ref()
                                .unwrap()
                                .job_template
                                .as_ref()
                                .unwrap()
                                .job_type_id
                                .clone(),
                            ScheduleStatusArg::from(ScheduleStatus::from(
                                s.raw_cached().unwrap().status(),
                            )),
                            s.raw_cached()
                                .unwrap()
                                .schedule
                                .as_ref()
                                .unwrap()
                                .labels
                                .iter()
                                .fold(String::new(), |mut acc, label| {
                                    if !acc.is_empty() {
                                        acc.push(',');
                                    }

                                    write!(&mut acc, "{}={}", label.key, label.value).unwrap();
                                    acc
                                }),
                        )
                    })
                    .try_collect::<Vec<_>>(),
            )
            .await
            .unwrap()
            .unwrap();

            jobs.sort_by(|(a, ..), (b, ..)| a.cmp(b));

            let mut candidates = Vec::with_capacity(jobs.len());

            for (i, (job_id, job_type, status, mut labels)) in jobs.into_iter().enumerate() {
                if !labels.is_empty() {
                    labels = format!(" [{labels}]");
                }

                candidates.push(
                    CompletionCandidate::new(job_id.to_string())
                        .help(Some(format!("{job_type} ({status}){labels}").into()))
                        .display_order(Some(i)),
                );
            }

            candidates
        })
}

pub(crate) fn complete_job_type(_: &OsStr) -> Vec<CompletionCandidate> {
    let Some(ora_url) = get_ora_url() else {
        return vec![];
    };

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let client = ora::AdminClient::new(AdminServiceClient::new(
                Endpoint::from_shared(ora_url).unwrap().connect_lazy(),
            ));

            let mut job_types = timeout(Duration::from_secs(10), client.list_job_types())
                .await
                .unwrap()
                .unwrap();

            job_types.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

            let mut candidates = Vec::with_capacity(job_types.len());

            for (i, job_type) in job_types.into_iter().enumerate() {
                candidates.push(
                    CompletionCandidate::new(job_type.id.to_string())
                        .help(job_type.description.map(Into::into))
                        .display_order(Some(i)),
                );
            }

            candidates
        })
}

/// Get the ora URL for completions.
///
/// Tries to look for an `--url` arg,
/// as well as the `ORA_URL` env var.
fn get_ora_url() -> Option<String> {
    #[derive(Parser)]
    #[command(ignore_errors(true))]
    struct OraArgs {
        #[arg(long, global = true)]
        url: Option<String>,
    }

    OraArgs::try_parse_from(std::env::args().skip(2))
        .ok()
        .and_then(|args| args.url)
        .or_else(|| std::env::var("ORA_URL").ok())
}
