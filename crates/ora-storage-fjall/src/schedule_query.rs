use std::cmp::Reverse;

use eyre::bail;
use fjall::ReadTransaction;
use ora_storage::{
    IndexMap, IndexSet, ScheduleLabelFilterValue, ScheduleQueryFilters, ScheduleQueryOrder,
    ScheduleQueryResult,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{indexes::ScheduleJobIndexKey, util::deserialize_systemtime};

use super::{indexes::LabelIndexKey, partitions::Partitions};

pub(super) fn query_schedule_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: ScheduleQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    let mut applied_filters = AppliedFilters::default();
    let mut schedule_candidates =
        collect_schedule_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;
    filter_schedule_candidates(
        tx,
        partitions,
        &mut schedule_candidates,
        &filters,
        &applied_filters,
    )?;

    Ok(schedule_candidates)
}

pub(super) fn count_schedules(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: ScheduleQueryFilters,
) -> eyre::Result<u64> {
    let mut applied_filters = AppliedFilters::default();
    let mut schedule_candidates =
        collect_schedule_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;
    filter_schedule_candidates(
        tx,
        partitions,
        &mut schedule_candidates,
        &filters,
        &applied_filters,
    )?;

    Ok(u64::try_from(schedule_candidates.len())?)
}

pub(super) fn query_schedules(
    tx: &ReadTransaction,
    partitions: &Partitions,
    cursor: Option<Cursor>,
    limit: usize,
    filters: ScheduleQueryFilters,
    order: ScheduleQueryOrder,
) -> eyre::Result<ScheduleQueryResult> {
    let mut applied_filters = AppliedFilters::default();
    let mut schedule_candidates =
        collect_schedule_candidate_ids(tx, partitions, &filters, &mut applied_filters)?;

    filter_schedule_candidates(
        tx,
        partitions,
        &mut schedule_candidates,
        &filters,
        &applied_filters,
    )?;
    sort_schedule_candidates(tx, partitions, &mut schedule_candidates, order);

    let last_schedule_id = cursor.and_then(|c| c.last_schedule_id);

    if let Some(last_schedule_id) = last_schedule_id {
        if let Some(last_schedule_index) = schedule_candidates
            .iter()
            .position(|&schedule_id| schedule_id == last_schedule_id)
        {
            schedule_candidates.drain(0..=last_schedule_index);
        }
    }

    let has_more = schedule_candidates.len() > limit;

    schedule_candidates.truncate(limit);

    let schedules = schedule_details(tx, partitions, &schedule_candidates)?;

    let new_cursor = Cursor {
        last_schedule_id: schedules
            .last()
            .map(|schedule| schedule.id)
            .or(last_schedule_id),
        filters,
        order,
    };

    Ok(ScheduleQueryResult {
        cursor: Some(serde_json::to_string(&new_cursor).unwrap()),
        schedules,
        has_more,
    })
}

fn collect_schedule_candidate_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: &ScheduleQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    if let Some(schedule_ids) = &filters.schedule_ids {
        schedule_candidates_by_ids(tx, partitions, schedule_ids, filters, applied_filters)
    } else if let Some(job_type_ids) = &filters.job_type_ids {
        schedule_candidates_by_job_type_ids(tx, partitions, job_type_ids, applied_filters)
    } else if let Some(label_filters) = &filters.labels {
        schedule_candidates_by_labels(tx, partitions, label_filters, applied_filters)
    } else if let Some(schedule_ids) = &filters.job_ids {
        schedule_candidates_by_job_ids(tx, partitions, schedule_ids, applied_filters)
    } else {
        schedule_candidates_all(tx, partitions, filters, applied_filters)
    }
}

fn schedule_candidates_by_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    schedule_ids: &IndexSet<Uuid>,
    filters: &ScheduleQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.schedule_ids = true;
    applied_filters.active = true;

    match filters.active {
        Some(true) => schedule_ids
            .iter()
            .filter_map(|schedule_id| {
                match partitions
                    .active_schedules
                    .read(tx)
                    .contains_key(schedule_id)
                {
                    Ok(true) => Some(Ok(*schedule_id)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e.into())),
                }
            })
            .collect(),
        Some(false) => schedule_ids
            .iter()
            .filter_map(|schedule_id| {
                match partitions
                    .inactive_schedules
                    .read(tx)
                    .contains_key(schedule_id)
                {
                    Ok(true) => Some(Ok(*schedule_id)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e.into())),
                }
            })
            .collect(),
        None => schedule_ids
            .iter()
            .filter_map(|schedule_id| {
                match partitions
                    .active_schedules
                    .read(tx)
                    .contains_key(schedule_id)
                {
                    Ok(true) => Some(Ok(*schedule_id)),
                    Ok(false) => match partitions
                        .inactive_schedules
                        .read(tx)
                        .contains_key(schedule_id)
                    {
                        Ok(true) => Some(Ok(*schedule_id)),
                        Ok(false) => None,
                        Err(e) => Some(Err(e.into())),
                    },
                    Err(e) => Some(Err(e.into())),
                }
            })
            .collect(),
    }
}

// FIXME(perf): do we need an index for this?
fn schedule_candidates_by_job_type_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    job_type_ids: &IndexSet<String>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.job_type_ids = true;

    let mut candidates = Vec::new();

    let search_partitions = [&partitions.active_schedules, &partitions.inactive_schedules];

    for partition in search_partitions {
        for result in partition.read(tx).iter() {
            let (_, schedule_data) = result?;
            let schedule_data = schedule_data.value();

            if let Some(job_type_id) = schedule_data.job_type_id.as_ref() {
                if job_type_ids.contains(job_type_id.as_str()) {
                    candidates.push(schedule_data.id);
                }
            }
        }
    }

    Ok(candidates)
}

fn schedule_candidates_by_labels(
    tx: &ReadTransaction,
    partitions: &Partitions,
    label_filters: &IndexMap<String, ScheduleLabelFilterValue>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.labels = true;

    let mut candidates: Option<IndexSet<Uuid>> = None;

    for (label_key, label_value) in label_filters {
        // We collect schedule IDs from multiple index keys.
        let mut index_schedule_ids = IndexSet::default();

        let label_prefix = match label_value {
            ScheduleLabelFilterValue::Exists => LabelIndexKey::new_prefix_key(label_key),
            ScheduleLabelFilterValue::Equals(label_value) => {
                LabelIndexKey::new_prefix_key_value(label_key, label_value)
            }
        };

        for result in partitions
            .idx_schedule_labels
            .read(tx)
            .prefix(&label_prefix)
        {
            let (index_key, _) = result?;
            index_schedule_ids.insert(index_key.value().id());
        }

        candidates = match candidates {
            Some(mut candidates) => {
                candidates.retain(|schedule_id| index_schedule_ids.contains(schedule_id));
                Some(candidates)
            }
            None => Some(index_schedule_ids),
        }
    }

    Ok(candidates.unwrap_or_default().into_iter().collect())
}

fn schedule_candidates_by_job_ids(
    tx: &ReadTransaction,
    partitions: &Partitions,
    job_ids: &IndexSet<Uuid>,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.job_ids = true;

    let mut schedule_ids = IndexSet::default();

    for schedule_id in job_ids {
        if let Some(job_id) = partitions.idx_job_schedule.read(tx).get(schedule_id)? {
            schedule_ids.insert(job_id.value());
        }
    }

    Ok(schedule_ids.into_iter().collect())
}

fn schedule_candidates_all(
    tx: &ReadTransaction,
    partitions: &Partitions,
    filters: &ScheduleQueryFilters,
    applied_filters: &mut AppliedFilters,
) -> eyre::Result<Vec<Uuid>> {
    applied_filters.active = true;
    match filters.active {
        Some(true) => partitions
            .active_schedules
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let schedule_id = key.value();
                Ok(schedule_id)
            })
            .collect(),
        Some(false) => partitions
            .inactive_schedules
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let schedule_id = key.value();
                Ok(schedule_id)
            })
            .collect(),
        None => partitions
            .active_schedules
            .read(tx)
            .keys()
            .map(|k| {
                let key = k?;
                let schedule_id = key.value();
                Ok(schedule_id)
            })
            .chain(partitions.inactive_schedules.read(tx).keys().map(|k| {
                let key = k?;
                let schedule_id = key.value();
                Ok(schedule_id)
            }))
            .collect(),
    }
}

fn filter_schedule_candidates(
    tx: &ReadTransaction,
    partitions: &Partitions,
    candidates: &mut Vec<Uuid>,
    filters: &ScheduleQueryFilters,
    applied_filters: &AppliedFilters,
) -> eyre::Result<()> {
    let AppliedFilters {
        schedule_ids: applied_schedule_ids,
        job_type_ids: applied_job_type_ids,
        job_ids: applied_job_ids,
        labels: applied_labels,
        active: applied_active,
    } = applied_filters;

    if !applied_schedule_ids {
        if let Some(schedule_ids) = &filters.schedule_ids {
            candidates.retain(|schedule_id| schedule_ids.contains(schedule_id));
        }
    }

    if !applied_active {
        match filters.active {
            Some(true) => candidates.retain(|schedule_id| {
                partitions
                    .active_schedules
                    .read(tx)
                    .contains_key(schedule_id)
                    .unwrap()
            }),
            Some(false) => candidates.retain(|schedule_id| {
                partitions
                    .inactive_schedules
                    .read(tx)
                    .contains_key(schedule_id)
                    .unwrap()
            }),
            None => {}
        }
    }

    if !applied_job_type_ids {
        if let Some(job_type_ids) = &filters.job_type_ids {
            candidates.retain(|schedule_id| {
                let schedule_data = partitions
                    .active_schedules
                    .read(tx)
                    .get(schedule_id)
                    .unwrap()
                    .or_else(|| {
                        partitions
                            .inactive_schedules
                            .read(tx)
                            .get(schedule_id)
                            .unwrap()
                    })
                    .unwrap();

                if let Some(job_type_id) = schedule_data.value().job_type_id.as_ref() {
                    return job_type_ids.contains(job_type_id.as_str());
                }

                false
            });
        }
    }

    if !applied_labels {
        if let Some(label_filters) = &filters.labels {
            for (label_key, label_value) in label_filters {
                let label_prefix = match label_value {
                    ScheduleLabelFilterValue::Exists => LabelIndexKey::new_prefix_key(label_key),
                    ScheduleLabelFilterValue::Equals(label_value) => {
                        LabelIndexKey::new_prefix_key_value(label_key, label_value)
                    }
                };

                let mut had_labels = false;

                for result in partitions
                    .idx_schedule_labels
                    .read(tx)
                    .prefix(&label_prefix)
                {
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

    if !applied_job_ids {
        if let Some(job_ids) = &filters.job_ids {
            candidates.retain(|schedule_id| {
                for result in partitions
                    .idx_schedule_jobs
                    .read(tx)
                    .prefix(&ScheduleJobIndexKey::new_prefix(*schedule_id))
                {
                    let (index_key, _) = result.unwrap();
                    if job_ids.contains(&index_key.value().job_id.unwrap()) {
                        return true;
                    }
                }

                false
            });
        }
    }

    Ok(())
}

fn sort_schedule_candidates(
    tx: &ReadTransaction,
    partitions: &Partitions,
    candidates: &mut [Uuid],
    order: ScheduleQueryOrder,
) {
    // Sort the IDs so that we are more likely to
    // get to have the data in memory.
    candidates.sort_unstable();

    match order {
        ScheduleQueryOrder::CreatedAtAsc => {
            candidates.sort_by_cached_key(|schedule_id| {
                partitions
                    .active_schedules
                    .read(tx)
                    .get(schedule_id)
                    .unwrap()
                    .or_else(|| {
                        partitions
                            .inactive_schedules
                            .read(tx)
                            .get(schedule_id)
                            .unwrap()
                    })
                    .unwrap()
                    .value()
                    .created_at
            });
        }
        ScheduleQueryOrder::CreatedAtDesc => {
            candidates.sort_by_cached_key(|schedule_id| {
                Reverse(
                    partitions
                        .active_schedules
                        .read(tx)
                        .get(schedule_id)
                        .unwrap()
                        .or_else(|| {
                            partitions
                                .inactive_schedules
                                .read(tx)
                                .get(schedule_id)
                                .unwrap()
                        })
                        .unwrap()
                        .value()
                        .created_at,
                )
            });
        }
    }
}

fn schedule_details(
    tx: &ReadTransaction,
    partitions: &Partitions,
    schedule_ids: &[Uuid],
) -> eyre::Result<Vec<ora_storage::ScheduleDetails>> {
    if schedule_ids.is_empty() {
        return Ok(Vec::new());
    }

    // We sort the IDs so it is more likely that the data is read in order.
    let mut sorted_schedule_ids = schedule_ids.to_vec();
    sorted_schedule_ids.sort();

    let mut schedule_details = Vec::with_capacity(sorted_schedule_ids.len());

    for schedule_id in sorted_schedule_ids {
        let mut active = false;

        let schedule_data =
            if let Some(schedule_data) = partitions.active_schedules.read(tx).get(&schedule_id)? {
                active = true;
                schedule_data
            } else if let Some(schedule_data) =
                partitions.inactive_schedules.read(tx).get(&schedule_id)?
            {
                schedule_data
            } else {
                bail!("schedule not found");
            };

        let schedule_data = schedule_data.value();

        schedule_details.push(ora_storage::ScheduleDetails {
            active,
            cancelled: schedule_data.cancelled_at.is_some(),
            id: schedule_id,
            labels: schedule_data
                .labels
                .iter()
                .map(|(key, value)| (key.as_ref().into(), value.as_ref().into()))
                .collect(),
            created_at: deserialize_systemtime(schedule_data.created_at),
            metadata_json: schedule_data
                .metadata_json
                .as_ref()
                .map(|s| s.as_str().into()),
            job_creation_policy: deserialize!(&schedule_data.job_creation_policy)?.into(),
            job_timing_policy: deserialize!(&schedule_data.job_timing_policy)?.into(),
            time_range: match schedule_data.time_range.as_ref() {
                Some(time_range) => Some(deserialize!(time_range)?.into()),
                None => None,
            },
        });
    }

    // We sort the schedule details by ID so the results are in the same order as the input IDs.
    schedule_details.sort_unstable_by_key(|schedule_details| {
        schedule_ids
            .iter()
            .position(|id| id == &schedule_details.id)
            .unwrap()
    });

    Ok(schedule_details)
}

/// Track which filters are already
/// applied to the query.
///
/// This allows us to avoid applying
/// the same filter multiple times.
#[derive(Debug, Default)]
struct AppliedFilters {
    schedule_ids: bool,
    job_type_ids: bool,
    job_ids: bool,
    labels: bool,
    active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) last_schedule_id: Option<Uuid>,
    pub(super) filters: ScheduleQueryFilters,
    pub(super) order: ScheduleQueryOrder,
}
