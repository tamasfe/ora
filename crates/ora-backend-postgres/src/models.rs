use ora_backend::executions::ExecutionStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i16)]
pub(crate) enum PgExecutionStatus {
    Pending = 0,
    InProgress = 1,
    Succeeded = 2,
    Failed = 3,
    Cancelled = 4,
}

impl From<i16> for PgExecutionStatus {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Pending,
            1 => Self::InProgress,
            2 => Self::Succeeded,
            3 => Self::Failed,
            4 => Self::Cancelled,
            _ => panic!("Invalid value for PgExecutionStatus: {value}"),
        }
    }
}

impl From<ExecutionStatus> for PgExecutionStatus {
    fn from(status: ExecutionStatus) -> Self {
        match status {
            ExecutionStatus::Pending => PgExecutionStatus::Pending,
            ExecutionStatus::Succeeded => PgExecutionStatus::Succeeded,
            ExecutionStatus::Failed => PgExecutionStatus::Failed,
            ExecutionStatus::Cancelled => PgExecutionStatus::Cancelled,
            ExecutionStatus::InProgress => PgExecutionStatus::InProgress,
        }
    }
}

impl From<PgExecutionStatus> for ExecutionStatus {
    fn from(status: PgExecutionStatus) -> Self {
        match status {
            PgExecutionStatus::Pending => ExecutionStatus::Pending,
            PgExecutionStatus::Succeeded => ExecutionStatus::Succeeded,
            PgExecutionStatus::Failed => ExecutionStatus::Failed,
            PgExecutionStatus::Cancelled => ExecutionStatus::Cancelled,
            PgExecutionStatus::InProgress => ExecutionStatus::InProgress,
        }
    }
}
