use std::cmp::Reverse;

use eyre::bail;
use fjall::ReadTransaction;
use ora_storage::{
    IndexMap, IndexSet, JobExecutionStatus, JobLabelFilterValue, JobQueryFilters, JobQueryOrder,
    JobQueryResult,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{indexes::ScheduleJobIndexKey, util::deserialize_systemtime};

use super::{
    indexes::{JobExecutionIndexKey, LabelIndexKey},
    partitions::Partitions,
};

pub(super) fn query_job_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: JobQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    let mut applied_filters = AppliedFilters::default();
    let mut job_candidates =
        collect_job_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;
    filter_job_candidates(
        tx,
        partitions,
        &mut job_candidates,
        &filters,
        &applied_filters,
    )?;

    Ok(job_candidates)
}

pub(super) fn count_jobs(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: JobQueryFilters,
) -> eyre::Result<u64> {
    let mut applied_filters = AppliedFilters::default();
    let mut job_candidates =
        collect_job_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;
    filter_job_candidates(
        tx,
        partitions,
        &mut job_candidates,
        &filters,
        &applied_filters,
    )?;

    Ok(u64::try_from(job_candidates.len())?)
}

pub(super) fn query_jobs(
    tx: &ReadTransaction,
    partitions: &Partitions,
    cursor: Option<Cursor>,
    limit: usize,
    filters: JobQueryFilters,
    order: JobQueryOrder,
) -> eyre::Result<JobQueryResult> {
    let mut applied_filters = AppliedFilters::default();
    let mut job_candidates =
        collect_job_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;

    filter_job_candidates(
        tx,
        partitions,
        &mut job_candidates,
        &filters,
        &applied_filters,
    )?;
    sort_job_candidates(tx, partitions, &mut job_candidates, order);

    let last_job_id = cursor.and_then(|c| c.last_job_id);

    if let Some(last_job_id) = last_job_id {
        if let Some(last_job_index) = job_candidates
            .iter()
            .position(|&job_id| job_id == last_job_id)
        {
            job_candidates.drain(0..=last_job_index);
        }
    }

    let has_more = job_candidates.len() > limit;

    job_candidates.truncate(limit);

    let jobs = job_details(tx, partitions, &job_candidates)?;

    let new_cursor = Cursor {
        last_job_id: jobs.last().map(|job| job.id).or(last_job_id),
        filters,
        order,
    };

    Ok(JobQueryResult {
        cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
        jobs,
        has_more,
    })
}

fn collect_job_candidate_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: &JobQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    if let Some(job_ids) = &filters.job_ids {
        job_candidates_by_ids(tx, partitions, job_ids, filters, applied_filters)
    } else if let Some(job_type_ids) = &filters.job_type_ids {
        job_candidates_by_job_type_ids(tx, partitions, job_type_ids, applied_filters)
    } else if let Some(execution_ids) = &filters.execution_ids {
        job_candidates_by_execution_ids(tx, partitions, execution_ids, applied_filters)
    } else if let Some(label_filters) = &filters.labels {
        job_candidates_by_labels(tx, partitions, label_filters, applied_filters)
    } else if let Some(schedule_ids) = &filters.schedule_ids {
        job_candidates_by_schedule_ids(tx, partitions, schedule_ids, applied_filters)
    } else {
        job_candidates_all(tx, partitions, filters, applied_filters)
    }
}

fn job_candidates_by_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    job_ids: &IndexSet<Uuid>,
    filters: &JobQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.job_ids = true;
    applied_filters.active = true;

    match filters.active {
        Some(true) => job_ids
            .iter()
            .filter_map(
                |job_id| match partitions.active_jobs.read(tx).contains_key(job_id) {
                    Ok(true) => Some(Ok(*job_id)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e.into())),
                },
            )
            .collect(),
        Some(false) => job_ids
            .iter()
            .filter_map(
                |job_id| match partitions.inactive_jobs.read(tx).contains_key(job_id) {
                    Ok(true) => Some(Ok(*job_id)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e.into())),
                },
            )
            .collect(),
        None => job_ids
            .iter()
            .filter_map(
                |job_id| match partitions.active_jobs.read(tx).contains_key(job_id) {
                    Ok(true) => Some(Ok(*job_id)),
                    Ok(false) => match partitions.inactive_jobs.read(tx).contains_key(job_id) {
                        Ok(true) => Some(Ok(*job_id)),
                        Ok(false) => None,
                        Err(e) => Some(Err(e.into())),
                    },
                    Err(e) => Some(Err(e.into())),
                },
            )
            .collect(),
    }
}

// FIXME(perf): do we need an index for this?
fn job_candidates_by_job_type_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    job_type_ids: &IndexSet<String>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.job_type_ids = true;

    let mut candidates = Vec::new();

    let search_partitions = [&partitions.active_jobs, &partitions.inactive_jobs];

    for partition in search_partitions {
        for result in partition.read(tx).iter() {
            let (_, job_data) = result?;
            let job_data = job_data.value();

            if job_type_ids.contains(job_data.job_type_id.as_str()) {
                candidates.push(job_data.id);
            }
        }
    }

    Ok(candidates)
}

fn job_candidates_by_execution_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    execution_ids: &IndexSet<Uuid>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.execution_ids = true;

    let mut job_ids = Vec::with_capacity(execution_ids.len());

    let search_partitions = [
        &partitions.failed_executions,
        &partitions.succeeded_executions,
        &partitions.running_executions,
        &partitions.assigned_executions,
        &partitions.ready_executions,
        &partitions.pending_executions,
    ];

    for execution_id in execution_ids {
        for partition in search_partitions {
            if let Some(execution_data) = partition.read(tx).get(execution_id)? {
                job_ids.push(execution_data.value().job_id);
                break;
            }
        }
    }

    Ok(job_ids)
}

fn job_candidates_by_labels(
    tx: &ReadTransaction,
    partitions: &Partitions,
    label_filters: &IndexMap<String, JobLabelFilterValue>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.labels = true;

    let mut candidates: Option<IndexSet<Uuid>> = None;

    for (label_key, label_value) in label_filters {
        // We collect job IDs from multiple index keys.
        let mut index_job_ids = IndexSet::default();

        let label_prefix = match label_value {
            JobLabelFilterValue::Exists => LabelIndexKey::new_prefix_key(label_key),
            JobLabelFilterValue::Equals(label_value) => {
                LabelIndexKey::new_prefix_key_value(label_key, label_value)
            }
        };

        for result in partitions.idx_job_labels.read(tx).prefix(&label_prefix) {
            let (index_key, _) = result?;
            index_job_ids.insert(index_key.value().id());
        }

        candidates = match candidates {
            Some(mut candidates) => {
                candidates.retain(|job_id| index_job_ids.contains(job_id));
                Some(candidates)
            }
            None => Some(index_job_ids),
        }
    }

    Ok(candidates.unwrap_or_default().into_iter().collect())
}

fn job_candidates_by_schedule_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    schedule_ids: &IndexSet<Uuid>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.schedule_ids = true;

    let mut job_ids = IndexSet::default();

    for schedule_id in schedule_ids {
        for result in partitions
            .idx_schedule_jobs
            .read(tx)
            .prefix(&ScheduleJobIndexKey::new_prefix(*schedule_id))
        {
            let (index_key, _) = result?;
            job_ids.insert(index_key.value().job_id.unwrap());
        }
    }

    Ok(job_ids.into_iter().collect())
}

fn job_candidates_all(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: &JobQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.active = true;
    match filters.active {
        Some(true) => partitions
            .active_jobs
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let job_id = key.value();
                Ok(job_id)
            })
            .collect(),
        Some(false) => partitions
            .inactive_jobs
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let job_id = key.value();
                Ok(job_id)
            })
            .collect(),
        None => partitions
            .active_jobs
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let job_id = key.value();
                Ok(job_id)
            })
            .chain(partitions.inactive_jobs.read(tx).keys().map(|k| {
                let key = k?;
                let job_id = key.value();
                Ok(job_id)
            }))
            .collect(),
    }
}

fn filter_job_candidates(
    tx: &ReadTransaction,
    partitions: &Partitions,
    candidates: &mut Vec<Uuid>,
    filters: &JobQueryFilters,
    applied_filters: &AppliedFilters,
) -> eyre::Result<()> {
    let AppliedFilters {
        job_ids: applied_job_ids,
        job_type_ids: applied_job_type_ids,
        schedule_ids: applied_schedule_ids,
        execution_ids: applied_execution_ids,
        labels: applied_labels,
        active: applied_active,
        status: applied_status,
    } = applied_filters;

    if !applied_job_ids {
        if let Some(job_ids) = &filters.job_ids {
            candidates.retain(|job_id| job_ids.contains(job_id));
        }
    }

    if !applied_active {
        match filters.active {
            Some(true) => candidates.retain(|job_id| {
                partitions
                    .active_jobs
                    .read(tx)
                    .contains_key(job_id)
                    .unwrap()
            }),
            Some(false) => candidates.retain(|job_id| {
                partitions
                    .inactive_jobs
                    .read(tx)
                    .contains_key(job_id)
                    .unwrap()
            }),
            None => {}
        }
    }

    if !applied_job_type_ids {
        if let Some(job_type_ids) = &filters.job_type_ids {
            candidates.retain(|job_id| {
                let job_data = partitions
                    .active_jobs
                    .read(tx)
                    .get(job_id)
                    .unwrap()
                    .or_else(|| partitions.inactive_jobs.read(tx).get(job_id).unwrap())
                    .unwrap();

                job_type_ids.contains(job_data.value().job_type_id.as_str())
            });
        }
    }

    if !applied_execution_ids {
        if let Some(execution_ids) = &filters.execution_ids {
            candidates.retain(|job_id| {
                let partition_tx = partitions.idx_job_executions.read(tx);
                let job_execution_ids =
                    partition_tx.prefix(&JobExecutionIndexKey::new_prefix(*job_id));

                for job_execution_id in job_execution_ids {
                    if execution_ids
                        .contains(&job_execution_id.unwrap().0.value().execution_id.unwrap())
                    {
                        return true;
                    }
                }

                false
            });
        }
    }

    if !applied_labels {
        if let Some(label_filters) = &filters.labels {
            for (label_key, label_value) in label_filters {
                let label_prefix = match label_value {
                    JobLabelFilterValue::Exists => LabelIndexKey::new_prefix_key(label_key),
                    JobLabelFilterValue::Equals(label_value) => {
                        LabelIndexKey::new_prefix_key_value(label_key, label_value)
                    }
                };

                let mut had_labels = false;

                for result in partitions.idx_job_labels.read(tx).prefix(&label_prefix) {
                    had_labels = true;

                    let (index_key, _) = result?;
                    candidates.retain(|&id| id == index_key.value().id());
                }

                if !had_labels {
                    candidates.clear();
                    break;
                }
            }
        }
    }

    if !applied_schedule_ids {
        if let Some(schedule_ids) = &filters.schedule_ids {
            candidates.retain(|job_id| {
                if let Some(schedule_id) = partitions.idx_job_schedule.read(tx).get(job_id).unwrap()
                {
                    schedule_ids.contains(&schedule_id.value())
                } else {
                    false
                }
            });
        }
    }

    if !applied_status {
        if let Some(status_filters) = &filters.execution_status {
            candidates.retain(|job_id| {
                if let Some(last_execution_id) = partitions
                    .idx_job_executions
                    .read(tx)
                    .prefix(&JobExecutionIndexKey::new_prefix(*job_id))
                    .last()
                    .map(|res| res.unwrap().0.value().execution_id.unwrap())
                {
                    let search_partitions = [
                        (&partitions.failed_executions, JobExecutionStatus::Failed),
                        (
                            &partitions.succeeded_executions,
                            JobExecutionStatus::Succeeded,
                        ),
                        (&partitions.running_executions, JobExecutionStatus::Running),
                        (
                            &partitions.assigned_executions,
                            JobExecutionStatus::Assigned,
                        ),
                        (&partitions.ready_executions, JobExecutionStatus::Ready),
                        (&partitions.pending_executions, JobExecutionStatus::Pending),
                    ];

                    for (partition, partition_status) in search_partitions {
                        if status_filters.contains(&partition_status)
                            && partition.read(tx).contains_key(&last_execution_id).unwrap()
                        {
                            return true;
                        }
                    }

                    false
                } else {
                    status_filters.contains(&JobExecutionStatus::Pending)
                }
            });
        }
    }

    Ok(())
}

fn sort_job_candidates(
    tx: &ReadTransaction,
    partitions: &Partitions,
    candidates: &mut [Uuid],
    order: JobQueryOrder,
) {
    // Sort the IDs so that we are more likely to
    // get to have the data in memory.
    candidates.sort_unstable();

    match order {
        JobQueryOrder::CreatedAtAsc => {
            candidates.sort_by_cached_key(|job_id| {
                partitions
                    .active_jobs
                    .read(tx)
                    .get(job_id)
                    .unwrap()
                    .or_else(|| partitions.inactive_jobs.read(tx).get(job_id).unwrap())
                    .unwrap()
                    .value()
                    .created_at
            });
        }
        JobQueryOrder::CreatedAtDesc => {
            candidates.sort_by_cached_key(|job_id| {
                Reverse(
                    partitions
                        .active_jobs
                        .read(tx)
                        .get(job_id)
                        .unwrap()
                        .or_else(|| partitions.inactive_jobs.read(tx).get(job_id).unwrap())
                        .unwrap()
                        .value()
                        .created_at,
                )
            });
        }
        JobQueryOrder::TargetExecutionTimeAsc => {
            candidates.sort_by_cached_key(|job_id| {
                partitions
                    .active_jobs
                    .read(tx)
                    .get(job_id)
                    .unwrap()
                    .or_else(|| partitions.inactive_jobs.read(tx).get(job_id).unwrap())
                    .unwrap()
                    .value()
                    .target_execution_time
            });
        }
        JobQueryOrder::TargetExecutionTimeDesc => {
            candidates.sort_by_cached_key(|job_id| {
                Reverse(
                    partitions
                        .active_jobs
                        .read(tx)
                        .get(job_id)
                        .unwrap()
                        .or_else(|| partitions.inactive_jobs.read(tx).get(job_id).unwrap())
                        .unwrap()
                        .value()
                        .target_execution_time,
                )
            });
        }
    }
}

fn job_details(
    tx: &ReadTransaction,
    partitions: &Partitions,
    job_ids: &[Uuid],
) -> eyre::Result<Vec<ora_storage::JobDetails>> {
    if job_ids.is_empty() {
        return Ok(Vec::new());
    }

    // We sort the IDs so it is more likely that the data is read in order.
    let mut sorted_job_ids = job_ids.to_vec();
    sorted_job_ids.sort();

    let mut job_details = Vec::with_capacity(sorted_job_ids.len());

    for job_id in sorted_job_ids {
        let mut active = false;

        let job_data = if let Some(job_data_bytes) = partitions.active_jobs.read(tx).get(&job_id)? {
            active = true;
            job_data_bytes
        } else if let Some(job_data_bytes) = partitions.inactive_jobs.read(tx).get(&job_id)? {
            job_data_bytes
        } else {
            bail!("job not found");
        };

        let job_data = job_data.value();

        let job_execution_ids = partitions
            .idx_job_executions
            .read(tx)
            .prefix(&JobExecutionIndexKey::new_prefix(job_id))
            .map(|res| res.unwrap().0.value().execution_id.unwrap())
            .collect::<Vec<_>>();

        job_details.push(ora_storage::JobDetails {
            active,
            cancelled: job_data.cancelled_at.is_some(),
            id: job_id,
            job_type_id: job_data.job_type_id.as_str().into(),
            schedule_id: job_data.schedule_id.as_ref().copied(),
            target_execution_time: deserialize_systemtime(job_data.target_execution_time),
            input_payload_json: job_data.input_payload_json.as_str().into(),
            labels: job_data
                .labels
                .iter()
                .map(|(key, value)| (key.as_ref().into(), value.as_ref().into()))
                .collect(),
            timeout_policy: deserialize_archived!(&job_data.timeout_policy)
                .unwrap()
                .into(),
            retry_policy: deserialize_archived!(&job_data.retry_policy)
                .unwrap()
                .into(),
            created_at: deserialize_systemtime(job_data.created_at),
            executions: execution_details(tx, partitions, &job_execution_ids)?,
            metadata_json: job_data.metadata_json.as_ref().map(|s| s.as_str().into()),
        });
    }

    // We sort the job details by ID so the results are in the same order as the input IDs.
    job_details.sort_unstable_by_key(|job_details| {
        job_ids.iter().position(|id| id == &job_details.id).unwrap()
    });

    Ok(job_details)
}

fn execution_details(
    tx: &ReadTransaction,
    partitions: &Partitions,
    execution_ids: &[Uuid],
) -> eyre::Result<Vec<ora_storage::ExecutionDetails>> {
    if execution_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut execution_details = Vec::with_capacity(execution_ids.len());

    for &execution_id in execution_ids {
        let search_partitions = [
            &partitions.pending_executions,
            &partitions.ready_executions,
            &partitions.assigned_executions,
            &partitions.running_executions,
            &partitions.succeeded_executions,
            &partitions.failed_executions,
        ];

        let mut found = false;
        for partition in search_partitions {
            if let Some(execution_data) = partition.read(tx).get(&execution_id)? {
                let execution_data = execution_data.value();

                execution_details.push(ora_storage::ExecutionDetails {
                    id: execution_id,
                    job_id: execution_data.job_id,
                    executor_id: execution_data.executor_id.as_ref().copied(),
                    created_at: deserialize_systemtime(execution_data.created_at),
                    ready_at: execution_data
                        .ready_at
                        .as_ref()
                        .copied()
                        .map(deserialize_systemtime),
                    assigned_at: execution_data
                        .assigned_at
                        .as_ref()
                        .copied()
                        .map(deserialize_systemtime),
                    started_at: execution_data
                        .started_at
                        .as_ref()
                        .copied()
                        .map(deserialize_systemtime),
                    succeeded_at: execution_data
                        .succeeded_at
                        .as_ref()
                        .copied()
                        .map(deserialize_systemtime),
                    failed_at: execution_data
                        .failed_at
                        .as_ref()
                        .copied()
                        .map(deserialize_systemtime),
                    output_payload_json: execution_data
                        .output_payload_json
                        .as_ref()
                        .map(|s| s.as_str().into()),
                    failure_reason: execution_data
                        .failure_reason
                        .as_ref()
                        .map(|s| s.as_str().into()),
                    status: if execution_data.failed_at.is_some() {
                        ora_storage::JobExecutionStatus::Failed
                    } else if execution_data.succeeded_at.is_some() {
                        ora_storage::JobExecutionStatus::Succeeded
                    } else if execution_data.started_at.is_some() {
                        ora_storage::JobExecutionStatus::Running
                    } else if execution_data.assigned_at.is_some() {
                        ora_storage::JobExecutionStatus::Assigned
                    } else if execution_data.ready_at.is_some() {
                        ora_storage::JobExecutionStatus::Ready
                    } else {
                        ora_storage::JobExecutionStatus::Pending
                    },
                });

                found = true;
                break;
            }
        }

        if !found {
            bail!("execution not found");
        }
    }

    // We sort the execution details by ID so the results are in the same order as the input IDs.
    execution_details.sort_unstable_by_key(|execution_details| {
        execution_ids
            .iter()
            .position(|id| id == &execution_details.id)
            .unwrap()
    });

    Ok(execution_details)
}

/// Track which filters are already
/// applied to the query.
///
/// This allows us to avoid applying
/// the same filter multiple times.
#[derive(Debug, Default)]
struct AppliedFilters {
    job_ids: bool,
    job_type_ids: bool,
    schedule_ids: bool,
    execution_ids: bool,
    labels: bool,
    active: bool,
    status: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_job_id: Option<Uuid>,
    pub(super) filters: JobQueryFilters,
    pub(super) order: JobQueryOrder,
}
