import json
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Any, Literal

from ora_client.job_definition import JobDefinition


@dataclass
class ScheduleDefinition:
    """
    Defines a schedule for creating jobs.
    """

    job_definition: JobDefinition
    """
    The definition to use for creating jobs.
    """

    after: datetime | None = None
    """
    Only create jobs after this time.
    """

    before: datetime | None = None
    """
    Only create jobs before this time.
    """

    propagate_labels: bool = True
    """
    Propagate labels to the created jobs.

    Note that this is done on the client side before
    creating the schedule.
    """

    labels: dict[str, str] = field(default_factory=dict)
    """
    Schedule labels.
    """

    cron_expression: str | None = None
    """
    The cron expression for the schedule.

    Note that this is mutually exclusive with `interval`.
    """

    repeat_every: timedelta | None = None
    """
    Repeat the job creation at this interval.
    
    Note that this is mutually exclusive with `cron_expression`.
    """

    immediate_job: bool = False
    """
    Whether to create jobs immediately.

    This takes `after` and `before` into account.
    """

    on_missed: Literal["skip", "create"] = "skip"
    """
    Whether to skip or create jobs for missed times.
    """

    metadata_json: str | None = None
    """
    Metadata for the schedule.
    """

    def only_after(self, after: datetime) -> "ScheduleDefinition":
        """
        Set the time after which to create jobs.
        """
        self.after = after
        return self

    def only_before(self, before: datetime) -> "ScheduleDefinition":
        """
        Set the time before which to create jobs.
        """
        self.before = before
        return self

    def do_not_propagate_labels(self) -> "ScheduleDefinition":
        """
        Do not propagate labels to the created jobs.
        """
        self.propagate_labels = False
        return self

    def with_labels(self, **labels: str) -> "ScheduleDefinition":
        """
        Add labels to the schedule.
        """
        self.labels.update(labels)
        return self

    def with_cron_expression(self, cron_expression: str) -> "ScheduleDefinition":
        """
        Set the cron expression for the schedule.
        """
        self.cron_expression = cron_expression
        return self

    def repeat(self, interval: timedelta) -> "ScheduleDefinition":
        """
        Repeat the job creation at the given interval.
        """
        self.repeat_every = interval
        return self

    def immediate(self, immediate: bool) -> "ScheduleDefinition":
        """
        Create jobs immediately.
        """
        self.immediate_job = immediate
        return self

    def replace_metadata(self, metadata: Any) -> "ScheduleDefinition":
        """
        Replace the metadata of the schedule.
        """
        self.metadata_json = json.dumps(metadata)
        return self
