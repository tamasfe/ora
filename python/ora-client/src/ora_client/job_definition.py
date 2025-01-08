import json
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from enum import IntEnum
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from ora_client.schedule_definition import ScheduleDefinition


@dataclass
class RetryPolicy:
    """
    A retry policy for a job.
    """

    retries: int = 0
    """
    The number of retries for the job.

    If the number of retries is zero, the job is not retried.
    """


class TimeoutBaseTime(IntEnum):
    """
    The base time for the timeout.
    """

    START_TIME = 2
    """
    The base time is the start time of the job.
    """

    TARGET_EXECUTION_TIME = 1
    """
    The base time is the target execution time of the job.

    Note that if the target execution time is not set,
    the timeout is calculated from the start time of the job.

    If the target execution time is in the past,
    the jobs may be immediately timed out.
    """


@dataclass
class TimeoutPolicy:
    """
    A timeout policy for a job.
    """

    timeout: timedelta = field(default_factory=timedelta)
    """
    The timeout for the job.
    """

    base_time: TimeoutBaseTime = TimeoutBaseTime.START_TIME
    """
    The base time for the timeout.

    The timeout is calculated from this time.
    """


@dataclass
class JobDefinition:
    job_type_id: str
    """
    The ID of the job type.
    """

    target_execution_time: datetime
    """
    The target execution time of the job.

    If not provided, it should be set to the current time.
    """

    input_payload_json: str
    """
    The job input payload JSON that is passed to the worker.
    """

    labels: dict[str, str] = field(default_factory=dict)
    """
    The labels of the job.
    """

    timeout_policy: TimeoutPolicy = field(default_factory=TimeoutPolicy)
    """
    The timeout policy of the job.
    """

    retry_policy: RetryPolicy = field(default_factory=RetryPolicy)
    """
    Retry policy for the job.
    """

    metadata_json: str | None = None

    def at(self, target_execution_time: datetime) -> "JobDefinition":
        """
        Set the target execution time of the job.
        """
        self.target_execution_time = target_execution_time
        return self

    def now(self) -> "JobDefinition":
        """
        Set the target execution time to the current time.
        """
        self.target_execution_time = datetime.now(UTC)
        return self

    def with_labels(self, **labels: str) -> "JobDefinition":
        """
        Add labels to the job.
        """
        self.labels.update(labels)
        return self

    def with_timeout(
        self,
        timeout: timedelta,
        base_time: TimeoutBaseTime = TimeoutBaseTime.START_TIME,
    ) -> "JobDefinition":
        """
        Set the timeout policy for the job.
        """
        self.timeout_policy = TimeoutPolicy(timeout=timeout, base_time=base_time)
        return self

    def with_retries(self, retries: int) -> "JobDefinition":
        """
        Set the retry policy for the job.
        """
        self.retry_policy = RetryPolicy(retries=retries)
        return self

    def replace_metadata(self, metadata: Any) -> "JobDefinition":
        """
        Replace the metadata of the job.

        The metadata must be JSON serializable.
        """
        self.metadata_json = json.dumps(metadata)
        return self

    def repeat(self, interval: timedelta) -> "ScheduleDefinition":
        """
        Create a schedule that repeats the job creation at the given interval.
        """
        from ora_client.schedule_definition import ScheduleDefinition

        return ScheduleDefinition(
            job_definition=self,
            repeat_every=interval,
        )

    def repeat_cron(self, cron_expression: str) -> "ScheduleDefinition":
        """
        Create a schedule that repeats the job creation at the given cron expression.
        """
        from ora_client.schedule_definition import ScheduleDefinition

        return ScheduleDefinition(
            job_definition=self,
            cron_expression=cron_expression,
        )
