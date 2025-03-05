import asyncio
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any, AsyncGenerator, Iterable, Literal, cast

from grpclib.client import Channel
from pydantic import BaseModel, TypeAdapter

from ora_client.job_definition import JobDefinition
from ora_client.proto.ora.common.v1 import (
    JobDefinition as JobDefinitionProto,
)
from ora_client.proto.ora.common.v1 import (
    JobLabel,
    JobRetryPolicy,
    JobTimeoutBaseTime,
    JobTimeoutPolicy,
    ScheduleJobCreationPolicy,
    ScheduleJobTimingPolicy,
    ScheduleJobTimingPolicyCron,
    ScheduleJobTimingPolicyRepeat,
    ScheduleLabel,
    ScheduleMissedTimePolicy,
    TimeRange,
)
from ora_client.proto.ora.common.v1 import (
    ScheduleDefinition as ScheduleDefinitionProto,
)
from ora_client.proto.ora.server.v1 import (
    AddJobsRequest,
    AdminServiceStub,
    CancelJobsRequest,
    CancelSchedulesRequest,
    CountJobsRequest,
    CountSchedulesRequest,
    CreateSchedulesRequest,
    DeleteInactiveJobsRequest,
    DeleteInactiveSchedulesRequest,
    Job,
    JobExecutionStatus,
    JobLabelFilter,
    JobQueryFilter,
    JobQueryOrder,
    LabelFilterExistCondition,
    ListExecutorsRequest,
    ListJobsRequest,
    ListJobTypesRequest,
    ListSchedulesRequest,
    Schedule,
    ScheduleLabelFilter,
    ScheduleQueryFilter,
)
from ora_client.schedule_definition import ScheduleDefinition


@dataclass
class JobTypeInfo:
    """Definition of a job type."""

    id: str
    """The ID of the job type."""

    name: str | None
    """The name of the job type."""

    description: str | None
    """The description of the job type."""

    input_schema_json: str | None
    """The input JSON schema of the job type."""

    output_schema_json: str | None
    """The output JSON schema of the job type."""


@dataclass
class ExecutorInfo:
    """Information about an executor."""

    id: str
    """The ID of the executor."""

    name: str
    """The name of the executor."""

    last_seen_at: datetime
    """The time the executor was last seen."""

    alive: bool
    """Whether the executor is alive."""

    supported_job_type_ids: list[str]
    """The job types supported by the executor."""

    max_concurrent_executions: int
    """The maximum number of concurrent job executions."""

    assigned_execution_ids: list[str]
    """A list of execution IDs assigned to the executor."""


class JobHandle:
    """
    A handle to a job.
    """

    def __init__(
        self, job_id: str, client: AdminServiceStub, cached_job: Job | None = None
    ):
        self._job_id = job_id
        self._client = client
        self._cached_details = cached_job

    @property
    def id(self) -> str:
        """
        The ID of the job.
        """
        return self._job_id

    async def details(self) -> Job:
        """
        Get the details of the job.
        """
        if self._cached_details is not None and not self._cached_details.active:
            return self._cached_details

        res = await self._client.list_jobs(
            ListJobsRequest(
                filter=JobQueryFilter(
                    active=True,
                    job_ids=[self._job_id],
                )
            )
        )

        if len(res.jobs) != 1:
            raise RuntimeError("job not found")

        self._cached_details = res.jobs[0]

        return self._cached_details

    async def result[T](
        self,
        cls: type[T],
        poll_interval_seconds: float = 1.0,
    ) -> T | None:
        """
        Get the result of the job and deserialize it
        with the given class.
        """

        while True:
            job = await self.details()

            if job.active:
                await asyncio.sleep(poll_interval_seconds)
                continue

            break

        try:
            last_execution = job.executions[-1]
        except IndexError:
            return None

        if last_execution.output_payload_json is None:
            return None

        if issubclass(cls, BaseModel):
            return cls.model_validate_json(last_execution.output_payload_json)

        return TypeAdapter(cls).validate_json(last_execution.output_payload_json)

    async def cancel(self) -> None:
        """
        Cancel the job.
        """
        await self._client.cancel_jobs(
            CancelJobsRequest(
                filter=JobQueryFilter(
                    active=True,
                    job_ids=[self._job_id],
                )
            )
        )


class ScheduleHandle:
    """
    A handle to a schedule.
    """

    def __init__(
        self,
        schedule_id: str,
        client: AdminServiceStub,
        cached_schedule: Schedule | None = None,
    ):
        self._schedule_id = schedule_id
        self._client = client
        self._cached_details = cached_schedule

    @property
    def id(self) -> str:
        """
        The ID of the schedule.
        """
        return self._schedule_id

    async def details(self) -> Schedule:
        """
        Get the details of the schedule.
        """
        if self._cached_details is not None and not self._cached_details.active:
            return self._cached_details

        res = await self._client.list_schedules(
            ListSchedulesRequest(
                filter=ScheduleQueryFilter(
                    schedule_ids=[self._schedule_id],
                )
            )
        )

        if len(res.schedules) != 1:
            raise RuntimeError("schedule not found")

        self._cached_details = res.schedules[0]

        return self._cached_details

    async def jobs(
        self,
        order: Literal[
            "created_asc", "created_desc", "target_asc", "target_desc"
        ] = "created_asc",
        active: bool | None = None,
        status: list["JobExecutionStatus"] | None = None,
    ) -> AsyncGenerator[JobHandle, None]:
        """
        Get the jobs created by the schedule.
        """
        admin_client = AdminClient(self._client.channel)

        async for job in admin_client.jobs(
            schedule_ids=[self._schedule_id],
            order=order,
            active=active,
            status=status,
        ):
            yield job

    async def cancel(self) -> None:
        """
        Cancel the schedule.
        """
        await self._client.cancel_schedules(
            CancelSchedulesRequest(
                filter=ScheduleQueryFilter(
                    schedule_ids=[self._schedule_id],
                )
            )
        )


class AdminClient:
    """
    A high-level client for the admin service.
    """

    def __init__(self, channel: Channel):
        self._channel = channel
        self._client = AdminServiceStub(channel)

    async def __aenter__(self):
        await self._channel.__aenter__()
        return self

    async def __aexit__(self, exc_type, exc, tb):
        await self._channel.__aexit__(exc_type, exc, tb)

    async def add_jobs(
        self, *jobs: JobDefinition | Iterable[JobDefinition]
    ) -> list[JobHandle]:
        """
        Add jobs to the job queue.
        """
        jobs_flat: list[JobDefinition] = []

        for job in jobs:
            if isinstance(job, Iterable):
                jobs_flat.extend(job)
            else:
                jobs_flat.append(job)

        res = await self._client.add_jobs(
            AddJobsRequest(
                jobs=[
                    JobDefinitionProto(
                        job_type_id=job.job_type_id,
                        target_execution_time=job.target_execution_time,
                        input_payload_json=job.input_payload_json,
                        labels=[
                            JobLabel(
                                key=label[0],
                                value=label[1],
                            )
                            for label in job.labels.items()
                        ],
                        timeout_policy=JobTimeoutPolicy(
                            timeout=job.timeout_policy.timeout,
                            base_time=JobTimeoutBaseTime(job.timeout_policy.base_time),
                        ),
                        retry_policy=JobRetryPolicy(job.retry_policy.retries),
                        metadata_json=job.metadata_json,
                    )
                    for job in jobs_flat
                ]
            )
        )

        if len(res.job_ids) != len(jobs_flat):
            raise RuntimeError("failed to create all jobs")

        return [JobHandle(job_id=job_id, client=self._client) for job_id in res.job_ids]

    def job(self, job_id: str) -> JobHandle:
        """
        Get a handle to a job.

        Note that this does not check if the job exists.
        """
        return JobHandle(job_id=job_id, client=self._client)

    async def jobs(
        self,
        job_ids: list[str] | None = None,
        job_type_ids: list[str] | None = None,
        execution_ids: list[str] | None = None,
        schedule_ids: list[str] | None = None,
        status: list["JobExecutionStatus"] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        order: Literal[
            "created_asc", "created_desc", "target_asc", "target_desc"
        ] = "created_asc",
        created_after: datetime | None = None,
        created_before: datetime | None = None,
        target_execution_after: datetime | None = None,
        target_execution_before: datetime | None = None,
        buffer_size: int = 100,
    ) -> AsyncGenerator[JobHandle, None]:
        """
        Retrieve jobs based on the given filters.

        The returned job handles will have their details cached.
        """
        filter = JobQueryFilter()

        if job_ids is not None:
            filter.job_ids = job_ids

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if execution_ids is not None:
            filter.execution_ids = execution_ids

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if status is not None:
            filter.status = status

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        JobLabelFilter(key=key, exists=LabelFilterExistCondition.EXISTS)
                    )
                else:
                    filter.labels.append(JobLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        if target_execution_after is not None:
            filter.target_execution_time.start = target_execution_after

        if target_execution_before is not None:
            filter.target_execution_time.end = target_execution_before

        match order:
            case "created_asc":
                order_option = JobQueryOrder.CREATED_AT_ASC
            case "created_desc":
                order_option = JobQueryOrder.CREATED_AT_DESC
            case "target_asc":
                order_option = JobQueryOrder.TARGET_EXECUTION_TIME_ASC
            case "target_desc":
                order_option = JobQueryOrder.TARGET_EXECUTION_TIME_DESC

        cursor = None

        while True:
            res = await self._client.list_jobs(
                ListJobsRequest(
                    filter=filter,
                    order=order_option,
                    cursor=cursor,
                    limit=buffer_size,
                )
            )

            for job in res.jobs:
                yield JobHandle(
                    job_id=job.id,
                    client=self._client,
                    cached_job=job,
                )

            if not res.has_more:
                break

            cursor = res.cursor

    async def job_count(
        self,
        job_ids: list[str] | None = None,
        job_type_ids: list[str] | None = None,
        execution_ids: list[str] | None = None,
        schedule_ids: list[str] | None = None,
        status: list["JobExecutionStatus"] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
        target_execution_after: datetime | None = None,
        target_execution_before: datetime | None = None,
    ) -> int:
        """
        Count jobs based on the given filters.
        """

        filter = JobQueryFilter()

        if job_ids is not None:
            filter.job_ids = job_ids

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if execution_ids is not None:
            filter.execution_ids = execution_ids

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if status is not None:
            filter.status = status

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        JobLabelFilter(key=key, exists=LabelFilterExistCondition.EXISTS)
                    )
                else:
                    filter.labels.append(JobLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        if target_execution_after is not None:
            filter.target_execution_time.start = target_execution_after

        if target_execution_before is not None:
            filter.target_execution_time.end = target_execution_before

        res = await self._client.count_jobs(
            CountJobsRequest(
                filter=filter,
            )
        )

        return res.count

    async def job_exists(
        self,
        job_ids: list[str] | None = None,
        job_type_ids: list[str] | None = None,
        execution_ids: list[str] | None = None,
        schedule_ids: list[str] | None = None,
        status: list["JobExecutionStatus"] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
        target_execution_after: datetime | None = None,
        target_execution_before: datetime | None = None,
    ) -> bool:
        """
        Check if jobs exist based on the given filters.
        """

        return (
            await self.job_count(
                job_ids=job_ids,
                job_type_ids=job_type_ids,
                execution_ids=execution_ids,
                schedule_ids=schedule_ids,
                status=status,
                labels=labels,
                active=active,
                created_after=created_after,
                created_before=created_before,
                target_execution_after=target_execution_after,
                target_execution_before=target_execution_before,
            )
            > 0
        )

    async def cancel_jobs(
        self,
        job_ids: list[str] | None = None,
        job_type_ids: list[str] | None = None,
        execution_ids: list[str] | None = None,
        schedule_ids: list[str] | None = None,
        status: list["JobExecutionStatus"] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
        target_execution_after: datetime | None = None,
        target_execution_before: datetime | None = None,
    ):
        """
        Cancel jobs based on the given filters.
        """
        filter = JobQueryFilter()

        if job_ids is not None:
            filter.job_ids = job_ids

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if execution_ids is not None:
            filter.execution_ids = execution_ids

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if status is not None:
            filter.status = status

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        JobLabelFilter(key=key, exists=LabelFilterExistCondition.EXISTS)
                    )
                else:
                    filter.labels.append(JobLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        if target_execution_after is not None:
            filter.target_execution_time.start = target_execution_after

        if target_execution_before is not None:
            filter.target_execution_time.end = target_execution_before

        await self._client.cancel_jobs(
            CancelJobsRequest(
                filter=filter,
            )
        )

    async def add_schedules(
        self, *schedules: ScheduleDefinition | Iterable[ScheduleDefinition]
    ) -> list[ScheduleHandle]:
        """
        Add schedules to the scheduler.
        """
        schedules_flat: list[ScheduleDefinition] = []

        for schedule in schedules:
            if isinstance(schedule, Iterable):
                schedules_flat.extend(schedule)
            else:
                schedules_flat.append(schedule)

        proto_schedules: list[ScheduleDefinitionProto] = []

        for schedule in schedules_flat:
            if schedule.cron_expression is None and schedule.repeat_every is None:
                raise ValueError(
                    "schedule must have a cron expression or repeat interval"
                )

            if schedule.propagate_labels:
                for label in schedule.labels.items():
                    if label[0] not in schedule.job_definition.labels:
                        schedule.job_definition.labels[label[0]] = label[1]

            proto_schedules.append(
                ScheduleDefinitionProto(
                    metadata_json=schedule.metadata_json,
                    time_range=TimeRange(
                        start=schedule.after
                        if schedule.after is not None
                        else datetime.fromtimestamp(0, UTC),
                        end=schedule.before
                        if schedule.before is not None
                        else datetime.fromtimestamp(0, UTC),
                    ),
                    job_creation_policy=ScheduleJobCreationPolicy(
                        job_definition=JobDefinitionProto(
                            job_type_id=schedule.job_definition.job_type_id,
                            target_execution_time=schedule.job_definition.target_execution_time,
                            input_payload_json=schedule.job_definition.input_payload_json,
                            labels=[
                                JobLabel(
                                    key=label[0],
                                    value=label[1],
                                )
                                for label in schedule.job_definition.labels.items()
                            ],
                            timeout_policy=JobTimeoutPolicy(
                                timeout=schedule.job_definition.timeout_policy.timeout,
                                base_time=JobTimeoutBaseTime(
                                    schedule.job_definition.timeout_policy.base_time
                                ),
                            ),
                            retry_policy=JobRetryPolicy(
                                schedule.job_definition.retry_policy.retries
                            ),
                            metadata_json=schedule.job_definition.metadata_json,
                        ),
                    ),
                    job_timing_policy=ScheduleJobTimingPolicy(
                        cron=ScheduleJobTimingPolicyCron(
                            cron_expression=schedule.cron_expression,
                            immediate=schedule.immediate_job,
                            missed_time_policy=ScheduleMissedTimePolicy.SKIP
                            if schedule.on_missed == "skip"
                            else ScheduleMissedTimePolicy.CREATE,
                        )
                    )
                    if schedule.cron_expression is not None
                    else ScheduleJobTimingPolicy(
                        repeat=ScheduleJobTimingPolicyRepeat(
                            interval=cast(Any, schedule.repeat_every),
                            immediate=schedule.immediate_job,
                            missed_time_policy=ScheduleMissedTimePolicy.SKIP
                            if schedule.on_missed == "skip"
                            else ScheduleMissedTimePolicy.CREATE,
                        )
                    ),
                    labels=[
                        ScheduleLabel(
                            key=label[0],
                            value=label[1],
                        )
                        for label in schedule.labels.items()
                    ],
                )
            )

        res = await self._client.create_schedules(
            CreateSchedulesRequest(
                schedules=proto_schedules,
            )
        )

        if len(res.schedule_ids) != len(schedules_flat):
            raise RuntimeError("failed to create all schedules")

        return [
            ScheduleHandle(schedule_id=schedule_id, client=self._client)
            for schedule_id in res.schedule_ids
        ]

    def schedule(self, schedule_id: str) -> ScheduleHandle:
        """
        Get a handle to a schedule.

        Note that this does not check if the schedule exists.
        """
        return ScheduleHandle(schedule_id=schedule_id, client=self._client)

    async def schedules(
        self,
        schedule_ids: list[str] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        job_type_ids: list[str] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
    ) -> AsyncGenerator[ScheduleHandle, None]:
        """
        Retrieve schedules based on the given filters.

        The returned schedule handles will have their details cached.
        """
        filter = ScheduleQueryFilter()

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        ScheduleLabelFilter(
                            key=key, exists=LabelFilterExistCondition.EXISTS
                        )
                    )
                else:
                    filter.labels.append(ScheduleLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        cursor = None

        while True:
            res = await self._client.list_schedules(
                ListSchedulesRequest(
                    filter=filter,
                    cursor=cursor,
                    limit=100,
                )
            )

            for schedule in res.schedules:
                yield ScheduleHandle(
                    schedule_id=schedule.id,
                    client=self._client,
                    cached_schedule=schedule,
                )

            if not res.has_more:
                break

            cursor = res.cursor

    async def cancel_schedules(
        self,
        schedule_ids: list[str] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        job_type_ids: list[str] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
    ):
        """
        Cancel schedules based on the given filters.
        """
        filter = ScheduleQueryFilter()

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        ScheduleLabelFilter(
                            key=key, exists=LabelFilterExistCondition.EXISTS
                        )
                    )
                else:
                    filter.labels.append(ScheduleLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        await self._client.cancel_schedules(
            CancelSchedulesRequest(
                filter=filter,
            )
        )

    async def schedule_count(
        self,
        schedule_ids: list[str] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        job_type_ids: list[str] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
    ) -> int:
        """
        Count schedules based on the given filters.
        """

        filter = ScheduleQueryFilter()

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        ScheduleLabelFilter(
                            key=key, exists=LabelFilterExistCondition.EXISTS
                        )
                    )
                else:
                    filter.labels.append(ScheduleLabelFilter(key=key, equals=value))

        if active is not None:
            filter.active = active

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        res = await self._client.count_schedules(
            CountSchedulesRequest(
                filter=filter,
            )
        )

        return res.count

    async def schedule_exists(
        self,
        schedule_ids: list[str] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        active: bool | None = None,
        job_type_ids: list[str] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
    ) -> bool:
        """
        Check if schedules exist based on the given filters.
        """

        return (
            await self.schedule_count(
                schedule_ids=schedule_ids,
                labels=labels,
                active=active,
                job_type_ids=job_type_ids,
                created_after=created_after,
                created_before=created_before,
            )
            > 0
        )

    async def remove_inactive_jobs(
        self,
        job_ids: list[str] | None = None,
        job_type_ids: list[str] | None = None,
        execution_ids: list[str] | None = None,
        schedule_ids: list[str] | None = None,
        status: list["JobExecutionStatus"] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
        target_execution_after: datetime | None = None,
        target_execution_before: datetime | None = None,
    ) -> None:
        """
        Remove inactive jobs.
        """
        filter = JobQueryFilter()

        if job_ids is not None:
            filter.job_ids = job_ids

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if execution_ids is not None:
            filter.execution_ids = execution_ids

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if status is not None:
            filter.status = status

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        JobLabelFilter(key=key, exists=LabelFilterExistCondition.EXISTS)
                    )
                else:
                    filter.labels.append(JobLabelFilter(key=key, equals=value))

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        if target_execution_after is not None:
            filter.target_execution_time.start = target_execution_after

        if target_execution_before is not None:
            filter.target_execution_time.end = target_execution_before

        await self._client.delete_inactive_jobs(
            DeleteInactiveJobsRequest(
                filter=filter,
            )
        )

    async def remove_inactive_schedules(
        self,
        schedule_ids: list[str] | None = None,
        labels: dict[str, str | Literal[True]] | None = None,
        job_type_ids: list[str] | None = None,
        created_after: datetime | None = None,
        created_before: datetime | None = None,
    ) -> None:
        """
        Remove inactive schedules.
        """
        filter = ScheduleQueryFilter()

        if schedule_ids is not None:
            filter.schedule_ids = schedule_ids

        if labels is not None:
            filter.labels = []

            for key, value in labels.items():
                if value is True:
                    filter.labels.append(
                        ScheduleLabelFilter(
                            key=key, exists=LabelFilterExistCondition.EXISTS
                        )
                    )
                else:
                    filter.labels.append(ScheduleLabelFilter(key=key, equals=value))

        if job_type_ids is not None:
            filter.job_type_ids = job_type_ids

        if created_after is not None:
            filter.created_at.start = created_after

        if created_before is not None:
            filter.created_at.end = created_before

        await self._client.delete_inactive_schedules(
            DeleteInactiveSchedulesRequest(
                filter=filter,
            )
        )

    async def job_types(self) -> list[JobTypeInfo]:
        """
        Retrieve all known job types.
        """

        res = await self._client.list_job_types(ListJobTypesRequest())

        return [
            JobTypeInfo(
                id=t.id,
                name=t.name,
                description=t.description,
                input_schema_json=t.input_schema_json,
                output_schema_json=t.output_schema_json,
            )
            for t in res.job_types
        ]

    async def executors(self) -> list[ExecutorInfo]:
        """
        Retrieve all executors that are connected to the server.
        """

        res = await self._client.list_executors(ListExecutorsRequest())

        return [
            ExecutorInfo(
                id=e.id,
                name=e.name,
                last_seen_at=e.last_seen_at,
                alive=e.alive,
                supported_job_type_ids=e.supported_job_type_ids,
                max_concurrent_executions=e.max_concurrent_executions,
                assigned_execution_ids=e.assigned_execution_ids,
            )
            for e in res.executors
        ]

    def inner(self):
        return self._client
