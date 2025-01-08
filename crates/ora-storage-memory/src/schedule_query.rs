use std::{cmp::Reverse, time::SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{MemoryStorage, Schedule};
use ora_storage::{
    IndexMap, IndexSet, ScheduleDetails, ScheduleLabelFilterValue, ScheduleQueryFilters,
    ScheduleQueryOrder, ScheduleQueryResult,
};

impl MemoryStorage {
    pub(super) fn query_schedule_ids_impl(&self, filters: ScheduleQueryFilters) -> Vec<Uuid> {
        let mut skip_filters = SkipFilters::default();

        let mut schedule_ids = self.collect_schedule_query_candidates(&filters, &mut skip_filters);
        self.filter_schedule_query_candidates(&mut schedule_ids, &filters, skip_filters);

        schedule_ids
    }

    pub(super) fn count_schedules_impl(&self, filters: ScheduleQueryFilters) -> u64 {
        let mut skip_filters = SkipFilters::default();

        let mut job_ids = self.collect_schedule_query_candidates(&filters, &mut skip_filters);
        self.filter_schedule_query_candidates(&mut job_ids, &filters, skip_filters);

        u64::try_from(job_ids.len()).unwrap_or(u64::MAX)
    }

    pub(super) fn query_schedules_impl(
        &self,
        cursor: Option<Cursor>,
        limit: usize,
        order: ScheduleQueryOrder,
        filters: ScheduleQueryFilters,
    ) -> ScheduleQueryResult {
        let mut skip_filters = SkipFilters::default();

        let mut schedule_candidates =
            self.collect_schedule_query_candidates(&filters, &mut skip_filters);

        self.filter_schedule_query_candidates(&mut schedule_candidates, &filters, skip_filters);
        self.sort_schedule_query_candidates(&mut schedule_candidates, order);

        if let Some(last_schedule_id) = cursor.and_then(|c| c.last_schedule_id) {
            if let Some(last_schedule_index) = schedule_candidates
                .iter()
                .position(|&job_id| job_id == last_schedule_id)
            {
                schedule_candidates.drain(0..=last_schedule_index);
            }
        }

        let has_more = schedule_candidates.len() > limit;

        schedule_candidates.truncate(limit);

        let schedules = self.collect_schedule_details(schedule_candidates, &filters);

        let new_cursor = Cursor {
            last_schedule_id: schedules.last().map(|schedule| schedule.id),
            filters,
            order,
        };

        ScheduleQueryResult {
            cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
            schedules,
            has_more,
        }
    }

    fn collect_schedule_query_candidates(
        &self,
        filters: &ScheduleQueryFilters,
        skip_filters: &mut SkipFilters,
    ) -> Vec<Uuid> {
        if let Some(schedule_ids) = &filters.schedule_ids {
            skip_filters.job_ids = true;
            skip_filters.active = true;
            self.schedule_candidates_by_ids(schedule_ids, filters)
        } else if let Some(job_ids) = &filters.job_ids {
            skip_filters.job_ids = true;
            self.schedule_candidates_by_job_ids(job_ids, filters)
        } else if let Some(job_type_ids) = &filters.job_type_ids {
            skip_filters.job_type_ids = true;
            skip_filters.active = true;
            self.schedule_candidates_by_job_type_ids(job_type_ids, filters)
        } else if let Some(label_filters) = &filters.labels {
            skip_filters.labels = true;
            skip_filters.active = true;
            self.schedule_candidates_by_labels(label_filters, filters)
        } else {
            skip_filters.active = true;
            self.all_schedule_candidates(filters)
        }
    }

    fn schedule_candidates_by_ids(
        &self,
        schedule_ids: &IndexSet<Uuid>,
        filters: &ScheduleQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_schedules = self.schedulable_schedules.read();
                schedule_ids
                    .iter()
                    .copied()
                    .filter(|job_id| active_schedules.contains_key(job_id))
                    .collect::<Vec<_>>()
            }
            Some(false) => {
                let inactive_schedules = self.unschedulable_schedules.read();
                schedule_ids
                    .iter()
                    .copied()
                    .filter(|job_id| inactive_schedules.contains_key(job_id))
                    .collect::<Vec<_>>()
            }
            None => {
                let active_schedules = self.schedulable_schedules.read();
                let inactive_schedules = self.unschedulable_schedules.read();
                schedule_ids
                    .iter()
                    .copied()
                    .filter(|job_id| {
                        active_schedules.contains_key(job_id)
                            || inactive_schedules.contains_key(job_id)
                    })
                    .collect::<Vec<_>>()
            }
        }
    }

    fn schedule_candidates_by_job_ids(
        &self,
        job_ids: &IndexSet<Uuid>,
        filters: &ScheduleQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            // Inactive schedules are associated with inactive jobs
            // only.
            Some(false) => {
                let inactive_jobs = self.unschedulable_jobs.read();

                job_ids
                    .iter()
                    .copied()
                    .filter_map(|job_id| {
                        if let Some(job) = inactive_jobs.get(&job_id) {
                            job.schedule_id
                        } else {
                            None
                        }
                    })
                    .collect::<IndexSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            }
            _ => {
                let active_jobs = self.schedulable_jobs.read();
                let inactive_jobs = self.unschedulable_jobs.read();

                job_ids
                    .iter()
                    .copied()
                    .filter_map(|job_id| {
                        if let Some(job) = active_jobs.get(&job_id) {
                            job.schedule_id
                        } else if let Some(job) = inactive_jobs.get(&job_id) {
                            job.schedule_id
                        } else {
                            None
                        }
                    })
                    .collect::<IndexSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            }
        }
    }

    fn schedule_candidates_by_job_type_ids(
        &self,
        job_type_ids: &IndexSet<String>,
        filters: &ScheduleQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_schedules = self.schedulable_schedules.read();
                active_schedules
                    .values()
                    .filter(|schedule| {
                        if let Some(job_type_id) = &schedule.job_type_id {
                            job_type_ids.contains(job_type_id)
                        } else {
                            false
                        }
                    })
                    .map(|schedule| schedule.id)
                    .collect::<Vec<_>>()
            }
            Some(false) => {
                let inactive_schedules = self.unschedulable_schedules.read();
                inactive_schedules
                    .values()
                    .filter(|schedule| {
                        if let Some(job_type_id) = &schedule.job_type_id {
                            job_type_ids.contains(job_type_id)
                        } else {
                            false
                        }
                    })
                    .map(|schedule| schedule.id)
                    .collect::<Vec<_>>()
            }
            None => {
                let active_schedules = self.schedulable_schedules.read();
                let inactive_schedules = self.unschedulable_schedules.read();
                active_schedules
                    .values()
                    .chain(inactive_schedules.values())
                    .filter(|schedule| {
                        if let Some(job_type_id) = &schedule.job_type_id {
                            job_type_ids.contains(job_type_id)
                        } else {
                            false
                        }
                    })
                    .map(|schedule| schedule.id)
                    .collect::<Vec<_>>()
            }
        }
    }

    fn schedule_candidates_by_labels(
        &self,
        label_filters: &IndexMap<String, ScheduleLabelFilterValue>,
        filters: &ScheduleQueryFilters,
    ) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_schedules = self.schedulable_schedules.read();

                active_schedules
                    .iter()
                    .filter(|(_, schedule)| labels_ok(label_filters, schedule))
                    .map(|(schedule_id, _)| *schedule_id)
                    .collect()
            }
            Some(false) => {
                let inactive_schedules = self.unschedulable_schedules.read();

                inactive_schedules
                    .iter()
                    .filter(|(_, schedule)| labels_ok(label_filters, schedule))
                    .map(|(schedule_id, _)| *schedule_id)
                    .collect()
            }
            None => {
                let active_schedules = self.schedulable_schedules.read();
                let inactive_schedules = self.unschedulable_schedules.read();

                active_schedules
                    .iter()
                    .filter(|(_, schedule)| labels_ok(label_filters, schedule))
                    .map(|(schedule_id, _)| *schedule_id)
                    .chain(
                        inactive_schedules
                            .iter()
                            .filter(|(_, schedule)| labels_ok(label_filters, schedule))
                            .map(|(schedule_id, _)| *schedule_id),
                    )
                    .collect()
            }
        }
    }

    fn all_schedule_candidates(&self, filters: &ScheduleQueryFilters) -> Vec<Uuid> {
        match filters.active {
            Some(true) => {
                let active_schedules = self.schedulable_schedules.read();
                active_schedules.keys().copied().collect()
            }
            Some(false) => {
                let inactive_schedules = self.unschedulable_schedules.read();
                inactive_schedules.keys().copied().collect()
            }
            None => {
                let active_schedules = self.schedulable_schedules.read();
                let inactive_schedules = self.unschedulable_schedules.read();
                active_schedules
                    .keys()
                    .copied()
                    .chain(inactive_schedules.keys().copied())
                    .collect()
            }
        }
    }

    fn filter_schedule_query_candidates(
        &self,
        schedule_ids: &mut Vec<Uuid>,
        filters: &ScheduleQueryFilters,
        skip: SkipFilters,
    ) {
        if !skip.active {
            if let Some(active) = filters.active {
                schedule_ids.retain(|job_id| {
                    if active {
                        self.schedulable_schedules.read().contains_key(job_id)
                    } else {
                        self.unschedulable_schedules.read().contains_key(job_id)
                    }
                });
            }
        }

        if !skip.schedule_ids {
            if let Some(filter_schedule_ids) = &filters.schedule_ids {
                schedule_ids.retain(|job_id| filter_schedule_ids.contains(job_id));
            }
        }

        if !skip.job_type_ids {
            if let Some(job_type_ids) = &filters.job_type_ids {
                schedule_ids.retain(|schedule_id| match filters.active {
                    Some(true) => self
                        .schedulable_schedules
                        .read()
                        .get(schedule_id)
                        .map(|schedule| {
                            schedule
                                .job_type_id
                                .as_ref()
                                .map_or(false, |id| job_type_ids.contains(id))
                        })
                        .unwrap_or(false),
                    Some(false) => self
                        .unschedulable_schedules
                        .read()
                        .get(schedule_id)
                        .map(|schedule| {
                            schedule
                                .job_type_id
                                .as_ref()
                                .map_or(false, |id| job_type_ids.contains(id))
                        })
                        .unwrap_or(false),
                    None => {
                        self.schedulable_schedules
                            .read()
                            .get(schedule_id)
                            .map(|schedule| {
                                schedule
                                    .job_type_id
                                    .as_ref()
                                    .map_or(false, |id| job_type_ids.contains(id))
                            })
                            .unwrap_or(false)
                            || self
                                .unschedulable_schedules
                                .read()
                                .get(schedule_id)
                                .map(|schedule| {
                                    schedule
                                        .job_type_id
                                        .as_ref()
                                        .map_or(false, |id| job_type_ids.contains(id))
                                })
                                .unwrap_or(false)
                    }
                });
            }
        }

        if !skip.job_ids {
            if let Some(job_ids) = &filters.job_ids {
                schedule_ids.retain(|schedule_id| {
                    self.schedulable_jobs.read().iter().any(|(_, job)| {
                        job.schedule_id == Some(*schedule_id) && job_ids.contains(&job.id)
                    }) || self.unschedulable_jobs.read().iter().any(|(_, job)| {
                        job.schedule_id == Some(*schedule_id) && job_ids.contains(&job.id)
                    })
                });
            }
        }

        if !skip.labels {
            if let Some(label_filters) = &filters.labels {
                schedule_ids.retain(|schedule_id| match filters.active {
                    Some(true) => {
                        let active_schedules = self.schedulable_schedules.read();

                        let Some(schedule) = active_schedules.get(schedule_id) else {
                            return false;
                        };

                        labels_ok(label_filters, schedule)
                    }
                    Some(false) => {
                        let inactive_schedules = self.unschedulable_schedules.read();

                        let Some(schedule) = inactive_schedules.get(schedule_id) else {
                            return false;
                        };

                        labels_ok(label_filters, schedule)
                    }
                    None => {
                        let active_schedules = self.schedulable_schedules.read();
                        let inactive_schedules = self.unschedulable_schedules.read();

                        let Some(schedule) = active_schedules
                            .get(schedule_id)
                            .or_else(|| inactive_schedules.get(schedule_id))
                        else {
                            return false;
                        };

                        labels_ok(label_filters, schedule)
                    }
                });
            }
        }
    }

    fn sort_schedule_query_candidates(&self, schedule_ids: &mut [Uuid], order: ScheduleQueryOrder) {
        match order {
            ScheduleQueryOrder::CreatedAtAsc => schedule_ids.sort_by_cached_key(|schedule_id| {
                self.schedulable_schedules
                    .read()
                    .get(schedule_id)
                    .map(|schedule| schedule.created_at)
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            }),
            ScheduleQueryOrder::CreatedAtDesc => {
                schedule_ids.sort_by_cached_key(|job_id| {
                    Reverse(
                        self.schedulable_jobs
                            .read()
                            .get(job_id)
                            .map(|job| job.created_at)
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                    )
                });
            }
        }
    }

    fn collect_schedule_details(
        &self,
        schedule_ids: Vec<Uuid>,
        filters: &ScheduleQueryFilters,
    ) -> Vec<ScheduleDetails> {
        schedule_ids
            .into_iter()
            .filter_map(|schedule_id| match filters.active {
                Some(true) => self
                    .schedulable_schedules
                    .read()
                    .get(&schedule_id)
                    .map(|schedule| self.schedule_details_of_schedule(schedule)),
                Some(false) => self
                    .unschedulable_schedules
                    .read()
                    .get(&schedule_id)
                    .map(|schedule| self.schedule_details_of_schedule(schedule)),
                None => self
                    .schedulable_schedules
                    .read()
                    .get(&schedule_id)
                    .map(|schedule| self.schedule_details_of_schedule(schedule))
                    .or_else(|| {
                        self.unschedulable_schedules
                            .read()
                            .get(&schedule_id)
                            .map(|schedule| self.schedule_details_of_schedule(schedule))
                    }),
            })
            .collect()
    }

    #[allow(clippy::unused_self)]
    fn schedule_details_of_schedule(&self, schedule: &Schedule) -> ScheduleDetails {
        let labels = schedule.labels.clone();

        ScheduleDetails {
            id: schedule.id,
            active: schedule.marked_unschedulable_at.is_none(),
            cancelled: schedule.cancelled_at.is_some(),
            created_at: schedule.created_at,
            job_timing_policy: schedule.job_timing_policy.clone(),
            job_creation_policy: schedule.job_creation_policy.clone(),
            time_range: schedule.time_range,
            labels,
            metadata_json: schedule.metadata_json.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_schedule_id: Option<Uuid>,
    pub(super) filters: ScheduleQueryFilters,
    pub(super) order: ScheduleQueryOrder,
}

#[derive(Debug, Default)]
pub(super) struct SkipFilters {
    pub(super) active: bool,
    pub(super) schedule_ids: bool,
    pub(super) job_ids: bool,
    pub(super) job_type_ids: bool,
    pub(super) labels: bool,
}

fn labels_ok(label_filters: &IndexMap<String, ScheduleLabelFilterValue>, job: &Schedule) -> bool {
    for (label_key, label_condition) in label_filters {
        let label_ok = match label_condition {
            ScheduleLabelFilterValue::Exists => job.labels.contains_key(label_key),
            ScheduleLabelFilterValue::Equals(expected_label_value) => {
                job.labels.get(label_key) == Some(expected_label_value)
            }
        };

        if !label_ok {
            return false;
        }
    }

    true
}
