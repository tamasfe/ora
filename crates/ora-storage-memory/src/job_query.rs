use std::{cmp::Reverse, time::SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ora_storage::{
    ExecutionDetails, IndexMap, IndexSet, JobDetails, JobExecutionStatus, JobLabelFilterValue,
    JobQueryFilters, JobQueryOrder, JobQueryResult,
};

use super::{Job, MemoryStorage};

impl MemoryStorage {
    pub(super) fn query_job_ids_impl(&self, filters: JobQueryFilters) -> Vec<Uuid> {
        let mut skip_filters = AppliedFilters::default();

        let mut job_ids = self.collect_job_query_candidates(&filters, &mut skip_filters);
        self.filter_job_query_candidates(&mut job_ids, &filters, skip_filters);

        job_ids
    }

    pub(super) fn count_jobs_impl(&self, filters: JobQueryFilters) -> u64 {
        let mut skip_filters = AppliedFilters::default();

        let mut job_ids = self.collect_job_query_candidates(&filters, &mut skip_filters);
        self.filter_job_query_candidates(&mut job_ids, &filters, skip_filters);

        u64::try_from(job_ids.len()).unwrap_or(u64::MAX)
    }

    pub(super) fn query_jobs_impl(
        &self,
        cursor: Option<Cursor>,
        limit: usize,
        order: JobQueryOrder,
        filters: JobQueryFilters,
    ) -> JobQueryResult {
        let mut skip_filters = AppliedFilters::default();

        let mut job_candidates = self.collect_job_query_candidates(&filters, &mut skip_filters);

        self.filter_job_query_candidates(&mut job_candidates, &filters, skip_filters);
        self.sort_job_query_candidates(&mut job_candidates, order);

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

        let jobs = self.collect_job_details(job_candidates, &filters);

        let new_cursor = Cursor {
            last_job_id: jobs.last().map(|job| job.id).or(last_job_id),
            filters,
            order,
        };

        JobQueryResult {
            cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
            jobs,
            has_more,
        }
    }

    fn collect_job_query_candidates(
        &self,
        filters: &JobQueryFilters,
        skip_filters: &mut AppliedFilters,
    ) -> Vec<Uuid> {
        if let Some(job_ids) = &filters.job_ids {
            skip_filters.job_ids = true;
            skip_filters.active = true;
            self.job_candidates_by_ids(job_ids, filters)
        } else if let Some(job_type_ids) = &filters.job_type_ids {
            skip_filters.job_type_ids = true;
            skip_filters.active = true;
            self.job_candidates_by_job_type_ids(job_type_ids, filters)
        } else if let Some(execution_ids) = &filters.execution_ids {
            skip_filters.execution_ids = true;
            self.job_candidates_by_execution_ids(execution_ids)
        } else if let Some(label_filters) = &filters.labels {
            skip_filters.labels = true;
            skip_filters.active = true;
            self.job_candidates_by_labels(label_filters, filters)
        } else if let Some(schedule_ids) = &filters.schedule_ids {
            skip_filters.active = true;
            skip_filters.schedule_ids = true;
            self.job_candidates_by_schedule_ids(schedule_ids, filters)
        } else {
            skip_filters.active = true;
            self.all_job_candidates(filters)
        }
    }

    fn all_job_candidates(&self, filters: &JobQueryFilters) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_jobs = self.schedulable_jobs.read();
                active_jobs.keys().copied().collect()
            }
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();
                inactive_jobs.keys().copied().collect()
            }
            None => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();
                active_jobs
                    .keys()
                    .copied()
                    .chain(inactive_jobs.keys().copied())
                    .collect()
            }
        }
    }

    fn job_candidates_by_ids(
        &self,
        job_ids: &IndexSet<Uuid>,
        filters: &JobQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_jobs = self.schedulable_jobs.read();
                job_ids
                    .iter()
                    .copied()
                    .filter(|job_id| active_jobs.contains_key(job_id))
                    .collect::<Vec<_>>()
            }
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();
                job_ids
                    .iter()
                    .copied()
                    .filter(|job_id| inactive_jobs.contains_key(job_id))
                    .collect::<Vec<_>>()
            }
            None => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();
                job_ids
                    .iter()
                    .copied()
                    .filter(|job_id| {
                        active_jobs.contains_key(job_id) || inactive_jobs.contains_key(job_id)
                    })
                    .collect::<Vec<_>>()
            }
        }
    }

    fn job_candidates_by_schedule_ids(
        &self,
        schedule_ids: &IndexSet<Uuid>,
        filters: &JobQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_jobs = self.schedulable_jobs.read();
                active_jobs
                    .iter()
                    .filter(|&(_, job)| {
                        if let Some(schedule_id) = &job.schedule_id {
                            schedule_ids.contains(schedule_id)
                        } else {
                            false
                        }
                    })
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();
                inactive_jobs
                    .iter()
                    .filter(|&(_, job)| {
                        if let Some(schedule_id) = &job.schedule_id {
                            schedule_ids.contains(schedule_id)
                        } else {
                            false
                        }
                    })
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            None => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();
                active_jobs
                    .iter()
                    .filter(|&(_, job)| {
                        if let Some(schedule_id) = &job.schedule_id {
                            schedule_ids.contains(schedule_id)
                        } else {
                            false
                        }
                    })
                    .map(|(job_id, _)| *job_id)
                    .chain(
                        inactive_jobs
                            .iter()
                            .filter(|&(_, job)| {
                                if let Some(schedule_id) = &job.schedule_id {
                                    schedule_ids.contains(schedule_id)
                                } else {
                                    false
                                }
                            })
                            .map(|(job_id, _)| *job_id),
                    )
                    .collect()
            }
        }
    }

    fn job_candidates_by_job_type_ids(
        &self,
        job_type_ids: &IndexSet<String>,
        filters: &JobQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_jobs = self.schedulable_jobs.read();
                active_jobs
                    .iter()
                    .filter(|&(_, job)| job_type_ids.contains(&job.job_type_id))
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();
                inactive_jobs
                    .iter()
                    .filter(|&(_, job)| job_type_ids.contains(&job.job_type_id))
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            None => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();
                active_jobs
                    .iter()
                    .filter(|&(_, job)| job_type_ids.contains(&job.job_type_id))
                    .map(|(job_id, _)| *job_id)
                    .chain(
                        inactive_jobs
                            .iter()
                            .filter(|&(_, job)| job_type_ids.contains(&job.job_type_id))
                            .map(|(job_id, _)| *job_id),
                    )
                    .collect()
            }
        }
    }

    fn job_candidates_by_execution_ids(&self, execution_ids: &IndexSet<Uuid>) -> Vec<Uuid> {
        self.pending_executions
            .read()
            .iter()
            .filter(|&(_, execution)| execution_ids.contains(&execution.id))
            .map(|(_, execution)| execution.job_id)
            .chain(
                self.ready_executions
                    .read()
                    .iter()
                    .filter(|&(_, execution)| execution_ids.contains(&execution.id))
                    .map(|(_, execution)| execution.job_id),
            )
            .chain(
                self.assigned_executions
                    .read()
                    .iter()
                    .filter(|&(_, execution)| execution_ids.contains(&execution.id))
                    .map(|(_, execution)| execution.job_id),
            )
            .chain(
                self.started_executions
                    .read()
                    .iter()
                    .filter(|&(_, execution)| execution_ids.contains(&execution.id))
                    .map(|(_, execution)| execution.job_id),
            )
            .chain(
                self.succeeded_executions
                    .read()
                    .iter()
                    .filter(|&(_, execution)| execution_ids.contains(&execution.id))
                    .map(|(_, execution)| execution.job_id),
            )
            .chain(
                self.failed_executions
                    .read()
                    .iter()
                    .filter(|&(_, execution)| execution_ids.contains(&execution.id))
                    .map(|(_, execution)| execution.job_id),
            )
            .collect()
    }

    fn job_candidates_by_labels(
        &self,
        label_filters: &IndexMap<String, JobLabelFilterValue>,
        filters: &JobQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_jobs = self.schedulable_jobs.read();

                active_jobs
                    .iter()
                    .filter(|(_, job)| labels_ok(label_filters, job))
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();

                inactive_jobs
                    .iter()
                    .filter(|(_, job)| labels_ok(label_filters, job))
                    .map(|(job_id, _)| *job_id)
                    .collect()
            }
            None => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();

                active_jobs
                    .iter()
                    .filter(|(_, job)| labels_ok(label_filters, job))
                    .map(|(job_id, _)| *job_id)
                    .chain(
                        inactive_jobs
                            .iter()
                            .filter(|(_, job)| labels_ok(label_filters, job))
                            .map(|(job_id, _)| *job_id),
                    )
                    .collect()
            }
        }
    }

    fn filter_job_query_candidates(
        &self,
        job_ids: &mut Vec<Uuid>,
        filters: &JobQueryFilters,
        applied: AppliedFilters,
    ) {
        let AppliedFilters {
            job_ids: applied_job_ids,
            job_type_ids: applied_job_type_ids,
            schedule_ids: applied_schedule_ids,
            execution_ids: applied_execution_ids,
            labels: applied_labels,
            active: applied_active,
        } = applied;

        if !applied_job_ids {
            if let Some(filter_job_ids) = &filters.job_ids {
                job_ids.retain(|job_id| filter_job_ids.contains(job_id));
            }
        }

        if !applied_active {
            if let Some(active) = filters.active {
                job_ids.retain(|job_id| {
                    if active {
                        self.schedulable_jobs.read().contains_key(job_id)
                    } else {
                        self.unschedulable_jobs.read().contains_key(job_id)
                    }
                });
            }
        }

        if !applied_job_type_ids {
            if let Some(job_type_ids) = &filters.job_type_ids {
                job_ids.retain(|job_id| match filters.active {
                    Some(true) => self
                        .schedulable_jobs
                        .read()
                        .get(job_id)
                        .map(|job| job_type_ids.contains(&job.job_type_id))
                        .unwrap_or(false),
                    Some(false) => self
                        .unschedulable_jobs
                        .read()
                        .get(job_id)
                        .map(|job| job_type_ids.contains(&job.job_type_id))
                        .unwrap_or(false),
                    None => {
                        self.schedulable_jobs
                            .read()
                            .get(job_id)
                            .map(|job| job_type_ids.contains(&job.job_type_id))
                            .unwrap_or(false)
                            || self
                                .unschedulable_jobs
                                .read()
                                .get(job_id)
                                .map(|job| job_type_ids.contains(&job.job_type_id))
                                .unwrap_or(false)
                    }
                });
            }
        }

        if !applied_execution_ids {
            if let Some(execution_ids) = &filters.execution_ids {
                job_ids.retain(|job_id| {
                    self.executions_by_job_id(*job_id)
                        .any(|execution_id| execution_ids.contains(&execution_id))
                });
            }
        }

        if !applied_schedule_ids {
            if let Some(schedule_ids) = &filters.schedule_ids {
                job_ids.retain(|job_id| match filters.active {
                    Some(true) => {
                        let active_jobs = self.schedulable_jobs.read();

                        let Some(job) = active_jobs.get(job_id) else {
                            return false;
                        };

                        job.schedule_id
                            .map_or(false, |schedule_id| schedule_ids.contains(&schedule_id))
                    }
                    Some(false) => {
                        let inactive_jobs = self.unschedulable_jobs.read();

                        let Some(job) = inactive_jobs.get(job_id) else {
                            return false;
                        };

                        job.schedule_id
                            .map_or(false, |schedule_id| schedule_ids.contains(&schedule_id))
                    }
                    None => {
                        let inactive_jobs = self.unschedulable_jobs.read();
                        let active_jobs = self.schedulable_jobs.read();

                        let Some(job) = active_jobs
                            .get(job_id)
                            .or_else(|| inactive_jobs.get(job_id))
                        else {
                            return false;
                        };

                        job.schedule_id
                            .map_or(false, |schedule_id| schedule_ids.contains(&schedule_id))
                    }
                });
            }
        }

        if let Some(status) = &filters.execution_status {
            job_ids.retain(|job_id| {
                if status.contains(&JobExecutionStatus::Pending) {
                    let include = self
                        .executions_by_job_id(*job_id)
                        .last()
                        .map(|execution_id| {
                            self.pending_executions.read().contains_key(&execution_id)
                        })
                        // No executions, so the job is pending.
                        .unwrap_or(true);

                    if include {
                        return true;
                    }
                }

                if status.contains(&JobExecutionStatus::Ready) {
                    let include = self
                        .executions_by_job_id(*job_id)
                        .last()
                        .map(|execution_id| {
                            self.ready_executions.read().contains_key(&execution_id)
                        })
                        .unwrap_or(false);

                    if include {
                        return true;
                    }
                }

                if status.contains(&JobExecutionStatus::Running) {
                    let include = self
                        .executions_by_job_id(*job_id)
                        .last()
                        .map(|execution_id| {
                            self.started_executions.read().contains_key(&execution_id)
                        })
                        .unwrap_or(false);

                    if include {
                        return true;
                    }
                }

                if status.contains(&JobExecutionStatus::Succeeded) {
                    let include = self
                        .executions_by_job_id(*job_id)
                        .last()
                        .map(|execution_id| {
                            self.succeeded_executions.read().contains_key(&execution_id)
                        })
                        .unwrap_or(false);

                    if include {
                        return true;
                    }
                }

                if status.contains(&JobExecutionStatus::Failed) {
                    let include = self
                        .executions_by_job_id(*job_id)
                        .last()
                        .map(|execution_id| {
                            self.failed_executions.read().contains_key(&execution_id)
                        })
                        .unwrap_or(false);

                    if include {
                        return true;
                    }
                }

                false
            });
        }

        if !applied_labels {
            if let Some(label_filters) = &filters.labels {
                job_ids.retain(|job_id| match filters.active {
                    Some(true) => {
                        let active_jobs = self.schedulable_jobs.read();

                        let Some(job) = active_jobs.get(job_id) else {
                            return false;
                        };

                        labels_ok(label_filters, job)
                    }
                    Some(false) => {
                        let inactive_jobs = self.unschedulable_jobs.read();

                        let Some(job) = inactive_jobs.get(job_id) else {
                            return false;
                        };

                        labels_ok(label_filters, job)
                    }
                    None => {
                        let inactive_jobs = self.unschedulable_jobs.read();
                        let active_jobs = self.schedulable_jobs.read();

                        let Some(job) = active_jobs
                            .get(job_id)
                            .or_else(|| inactive_jobs.get(job_id))
                        else {
                            return false;
                        };

                        labels_ok(label_filters, job)
                    }
                });
            }
        }
    }

    fn sort_job_query_candidates(&self, job_ids: &mut [Uuid], order: JobQueryOrder) {
        match order {
            JobQueryOrder::CreatedAtAsc => job_ids.sort_by_cached_key(|job_id| {
                self.schedulable_jobs
                    .read()
                    .get(job_id)
                    .map(|job| job.created_at)
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            }),
            JobQueryOrder::CreatedAtDesc => {
                job_ids.sort_by_cached_key(|job_id| {
                    Reverse(
                        self.schedulable_jobs
                            .read()
                            .get(job_id)
                            .map(|job| job.created_at)
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                    )
                });
            }
            JobQueryOrder::TargetExecutionTimeAsc => {
                job_ids.sort_by_cached_key(|job_id| {
                    self.schedulable_jobs
                        .read()
                        .get(job_id)
                        .map(|job| job.target_execution_time)
                        .unwrap_or(SystemTime::UNIX_EPOCH)
                });
            }
            JobQueryOrder::TargetExecutionTimeDesc => {
                job_ids.sort_by_cached_key(|job_id| {
                    Reverse(
                        self.schedulable_jobs
                            .read()
                            .get(job_id)
                            .map(|job| job.target_execution_time)
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                    )
                });
            }
        }
    }

    fn collect_job_details(
        &self,
        job_ids: Vec<Uuid>,
        filters: &JobQueryFilters,
    ) -> Vec<JobDetails> {
        job_ids
            .into_iter()
            .filter_map(|job_id| match filters.active {
                Some(true) => {
                    let active_jobs = self.schedulable_jobs.read();

                    active_jobs
                        .get(&job_id)
                        .map(|job| self.job_details_of_job(job))
                }
                Some(false) => {
                    let inactive_jobs = self.unschedulable_jobs.read();

                    inactive_jobs
                        .get(&job_id)
                        .map(|job| self.job_details_of_job(job))
                }
                None => {
                    let active_jobs = self.schedulable_jobs.read();
                    let inactive_jobs = self.unschedulable_jobs.read();

                    active_jobs
                        .get(&job_id)
                        .or_else(|| inactive_jobs.get(&job_id))
                        .map(|job| self.job_details_of_job(job))
                }
            })
            .collect()
    }

    fn job_details_of_job(&self, job: &Job) -> JobDetails {
        let labels = job.labels.clone();

        JobDetails {
            id: job.id,
            schedule_id: job.schedule_id,
            active: job.marked_unschedulable_at.is_none(),
            cancelled: job.cancelled_at.is_some(),
            created_at: job.created_at,
            job_type_id: job.job_type_id.clone(),
            input_payload_json: job.input_payload_json.clone(),
            target_execution_time: job.target_execution_time,
            retry_policy: job.retry_policy,
            timeout_policy: job.timeout_policy,
            labels,
            metadata_json: job.metadata_json.clone(),
            executions: self
                .executions_by_job_id(job.id)
                .filter_map(|execution_id| {
                    // We don't track the status within an execution,
                    // so we go through all phases.
                    self.pending_executions
                        .read()
                        .get(&execution_id)
                        .map(ExecutionDetails::from)
                        .or_else(|| {
                            self.ready_executions
                                .read()
                                .get(&execution_id)
                                .map(ExecutionDetails::from)
                        })
                        .or_else(|| {
                            self.assigned_executions
                                .read()
                                .get(&execution_id)
                                .map(ExecutionDetails::from)
                        })
                        .or_else(|| {
                            self.started_executions
                                .read()
                                .get(&execution_id)
                                .map(ExecutionDetails::from)
                        })
                        .or_else(|| {
                            self.succeeded_executions
                                .read()
                                .get(&execution_id)
                                .map(ExecutionDetails::from)
                        })
                        .or_else(|| {
                            self.failed_executions
                                .read()
                                .get(&execution_id)
                                .map(ExecutionDetails::from)
                        })
                })
                .collect(),
        }
    }
}

fn labels_ok(label_filters: &IndexMap<String, JobLabelFilterValue>, job: &Job) -> bool {
    for (label_key, label_condition) in label_filters {
        let label_ok = match label_condition {
            JobLabelFilterValue::Exists => job.labels.contains_key(label_key),
            JobLabelFilterValue::Equals(expected_label_value) => {
                job.labels.get(label_key) == Some(expected_label_value)
            }
        };

        if !label_ok {
            return false;
        }
    }

    true
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_job_id: Option<Uuid>,
    pub(super) filters: JobQueryFilters,
    pub(super) order: JobQueryOrder,
}

/// Filters to skip during the query.
///
/// This is used to avoid unnecessary work when filtering jobs.
#[derive(Debug, Default)]
struct AppliedFilters {
    job_ids: bool,
    job_type_ids: bool,
    schedule_ids: bool,
    execution_ids: bool,
    labels: bool,
    active: bool,
}
