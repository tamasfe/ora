use crate::{
    indexes::ScheduleJobIndexKey,
    models::{ExecutionData, JobData, JobTypeData, ScheduleData},
    typed::TxPartition,
};
use fjall::{PartitionCreateOptions, TxKeyspace};
use uuid::Uuid;

use super::indexes::{JobExecutionIndexKey, LabelIndexKey};

// NOTE: DO NOT RENAME PARTITIONS as the underlying storage is dependent on the names.
#[derive(Debug, Clone)]
pub(super) struct Partitions {
    // Job types
    pub(super) job_types: TxPartition<String, JobTypeData>,

    // Jobs
    pub(super) active_jobs: TxPartition<Uuid, JobData>,
    pub(super) inactive_jobs: TxPartition<Uuid, JobData>,

    // Executions
    pub(super) pending_executions: TxPartition<Uuid, ExecutionData>,
    pub(super) ready_executions: TxPartition<Uuid, ExecutionData>,
    pub(super) assigned_executions: TxPartition<Uuid, ExecutionData>,
    pub(super) running_executions: TxPartition<Uuid, ExecutionData>,
    pub(super) succeeded_executions: TxPartition<Uuid, ExecutionData>,
    pub(super) failed_executions: TxPartition<Uuid, ExecutionData>,

    // Schedules
    pub(super) active_schedules: TxPartition<Uuid, ScheduleData>,
    pub(super) inactive_schedules: TxPartition<Uuid, ScheduleData>,

    // Indexes
    pub(super) idx_job_labels: TxPartition<LabelIndexKey, ()>,
    pub(super) idx_job_executions: TxPartition<JobExecutionIndexKey, ()>,
    pub(super) idx_pending_jobs: TxPartition<Uuid, ()>,
    pub(super) idx_job_active_execution: TxPartition<Uuid, Uuid>,

    pub(super) idx_schedule_labels: TxPartition<LabelIndexKey, ()>,
    pub(super) idx_pending_schedules: TxPartition<Uuid, ()>,
    pub(super) idx_schedule_jobs: TxPartition<ScheduleJobIndexKey, ()>,
    pub(super) idx_schedule_active_job: TxPartition<Uuid, Uuid>,
    pub(super) idx_job_schedule: TxPartition<Uuid, Uuid>,
}

/// Options for each partition as a separate struct
/// for the sake of readability and it is easier to
/// expose the options to the user in the future.
#[derive(Default)]
struct PartitionOptions {
    job_types: PartitionCreateOptions,
    active_jobs: PartitionCreateOptions,
    inactive_jobs: PartitionCreateOptions,
    pending_executions: PartitionCreateOptions,
    ready_executions: PartitionCreateOptions,
    assigned_executions: PartitionCreateOptions,
    running_executions: PartitionCreateOptions,
    succeeded_executions: PartitionCreateOptions,
    failed_executions: PartitionCreateOptions,
    active_schedules: PartitionCreateOptions,
    inactive_schedules: PartitionCreateOptions,
    idx_job_labels: PartitionCreateOptions,
    idx_job_executions: PartitionCreateOptions,
    idx_pending_jobs: PartitionCreateOptions,
    idx_job_active_execution: PartitionCreateOptions,
    idx_pending_schedules: PartitionCreateOptions,
    idx_schedule_jobs: PartitionCreateOptions,
    idx_schedule_active_job: PartitionCreateOptions,
    idx_schedule_labels: PartitionCreateOptions,
    idx_job_schedule: PartitionCreateOptions,
}

impl Partitions {
    pub(super) fn new(keyspace: &TxKeyspace) -> fjall::Result<Self> {
        let options = PartitionOptions::default();

        macro_rules! create_partitions {
            ($($partition_name:ident),*$(,)?) => {
                Ok(Self {
                    $($partition_name: keyspace.open_partition(stringify!($partition_name), options.$partition_name)?.into(),)*
                })
            };
        }

        // NOTE: DO NOT RENAME PARTITIONS as the underlying storage is dependent on the names.
        create_partitions!(
            job_types,
            active_jobs,
            inactive_jobs,
            pending_executions,
            ready_executions,
            assigned_executions,
            running_executions,
            succeeded_executions,
            failed_executions,
            active_schedules,
            inactive_schedules,
            idx_job_labels,
            idx_job_executions,
            idx_pending_jobs,
            idx_job_active_execution,
            idx_pending_schedules,
            idx_schedule_jobs,
            idx_schedule_active_job,
            idx_schedule_labels,
            idx_job_schedule,
        )
    }
}
