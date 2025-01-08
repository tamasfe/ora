import asyncio
import traceback
from datetime import UTC, datetime
from typing import Any, Awaitable, Callable, Self, cast, get_type_hints

from betterproto import which_one_of
from grpclib.client import Channel
from loguru import logger
from pydantic import TypeAdapter

from ora_client.job_type import JobType
from ora_client.proto.ora.common.v1 import JobType as JobTypeProto
from ora_client.proto.ora.server.v1 import (
    ExecutionCancelled,
    ExecutionFailed,
    ExecutionReady,
    ExecutionStarted,
    ExecutionSucceeded,
    ExecutorCapabilities,
    ExecutorConnectionRequest,
    ExecutorHeartbeat,
    ExecutorMessage,
    ExecutorProperties,
    ExecutorServiceStub,
)


class ExecutionContext:
    def __init__(
        self,
        execution_id: str,
        job_id: str,
        job_type_id: str,
        target_execution_time: datetime,
        attempt_number: int,
    ):
        self._execution_id = execution_id
        self._job_id = job_id
        self._job_type_id = job_type_id
        self._target_execution_time = target_execution_time
        self._attempt_number = attempt_number

    @property
    def execution_id(self) -> str:
        return self._execution_id

    @property
    def job_id(self) -> str:
        return self._job_id

    @property
    def job_type_id(self) -> str:
        return self._job_type_id

    @property
    def target_execution_time(self) -> datetime:
        return self._target_execution_time

    @property
    def attempt_number(self) -> int:
        return self._attempt_number


def handler[I, O](
    func: Callable[[ExecutionContext, I], Awaitable[O]],
) -> Callable[[ExecutionContext, I], Awaitable[O]]:
    """
    A decorator to mark a function as a handler for a job type.

    This decorator makes sure that the function signature is correct.
    """

    ty_hints = get_type_hints(func)
    ty_hints_iter = iter(ty_hints.items())

    first_hint = next(ty_hints_iter)

    if first_hint[1] != ExecutionContext:
        raise TypeError("invalid handler: first argument must be ExecutionContext")

    second_hint = next(ty_hints_iter)

    if not issubclass(second_hint[1], JobType):
        raise TypeError("invalid handler: second argument must be a JobType")

    output_type: type | None = getattr(
        cast(Any, second_hint[1]), "__ora_output_type__", None
    )

    ret_type = ty_hints.get("return", None)

    if output_type is None and ret_type is not None:
        raise TypeError(f"handler output type must be None, got {ret_type}")

    if output_type is not ret_type:
        raise TypeError(
            f"handler output type differs from job type output type, expected {output_type}, got {ret_type}"
        )

    return func


class Executor:
    def __init__(self, max_concurrent: int = 1, name: str | None = None):
        self._name = name
        self._capacity = max_concurrent
        self._handlers: dict[str, _ExecutionHandler] = {}

    def handle[O](
        self,
        job_type: type[JobType[O]],
        fn: Callable[[ExecutionContext, Any], Awaitable[O]],
    ) -> Self:
        async def _handler(context: ExecutionContext, input_json: str) -> str:
            input = job_type.model_validate_json(input_json)
            output = await fn(context, input)

            adapter: TypeAdapter[Any] = TypeAdapter(
                getattr(job_type, "__ora_output_type__", None)
            )
            return adapter.dump_json(output, by_alias=True).decode()

        self._handlers[job_type.__ora_job_type_id__()] = _ExecutionHandler(
            job_type, _handler
        )
        return self

    async def run(self, channel: Channel):
        state = _ExecutorState()
        try:
            client = ExecutorServiceStub(channel)
            executor_messages: asyncio.Queue[ExecutorMessage] = asyncio.Queue()

            async def _heartbeat_loop():
                while True:
                    await asyncio.sleep(state.heartbeat_interval_seconds)

                    if state.shutting_down:
                        break

                    await executor_messages.put(
                        ExecutorMessage(
                            heartbeat=ExecutorHeartbeat(),
                        )
                    )

            asyncio.create_task(_heartbeat_loop())

            async def _executor_messages():
                yield ExecutorConnectionRequest(
                    message=ExecutorMessage(
                        capabilities=ExecutorCapabilities(
                            name=self._name or "",
                            max_concurrent_executions=self._capacity,
                            supported_job_types=[
                                JobTypeProto(
                                    id=n.job_type.__ora_job_type_id__(),
                                    description=n.job_type.__ora_job_type_description__(),
                                    input_schema_json=n.job_type.__ora_input_schema_json__(),
                                    output_schema_json=n.job_type.__ora_output_schema_json__(),
                                    name=n.job_type.__ora_job_type_id__(),
                                )
                                for n in self._handlers.values()
                            ],
                        )
                    )
                )

                while not state.shutting_down:
                    try:
                        message = await asyncio.wait_for(
                            executor_messages.get(),
                            timeout=5,
                        )
                    except asyncio.TimeoutError:
                        continue
                    executor_messages.task_done()
                    yield ExecutorConnectionRequest(message=message)

            server_messages = client.executor_connection(_executor_messages())

            async for resp in server_messages:
                message = resp.message
                if message is None:
                    continue

                match which_one_of(message, "server_message_kind"):
                    case _, ExecutorProperties(
                        executor_id=executor_id,
                        max_heartbeat_interval=max_heartbeat_interval,
                    ):
                        state.executor_id = executor_id
                        state.heartbeat_interval_seconds = (
                            max_heartbeat_interval.total_seconds() / 2
                        )
                    case _, ExecutionReady(
                        job_id,
                        execution_id,
                        job_type_id,
                        attempt_number,
                        input_payload_json,
                        target_execution_time,
                    ):
                        handler = self._handlers.get(job_type_id)

                        async def _run_execution():
                            try:
                                await executor_messages.put(
                                    ExecutorMessage(
                                        execution_started=ExecutionStarted(
                                            execution_id=execution_id,
                                            timestamp=datetime.now(UTC),
                                        )
                                    )
                                )

                                if handler is None:
                                    logger.error(
                                        "no handler for job type",
                                        job_type_id=job_type_id,
                                    )
                                    await executor_messages.put(
                                        ExecutorMessage(
                                            execution_failed=ExecutionFailed(
                                                error_message="unknown job type",
                                                execution_id=execution_id,
                                                timestamp=datetime.now(UTC),
                                            )
                                        )
                                    )
                                    return

                                context = ExecutionContext(
                                    execution_id=execution_id,
                                    job_id=job_id,
                                    job_type_id=job_type_id,
                                    target_execution_time=target_execution_time,
                                    attempt_number=attempt_number,
                                )

                                try:
                                    output_payload_json = await handler.fn(
                                        context, input_payload_json
                                    )
                                    await executor_messages.put(
                                        ExecutorMessage(
                                            execution_succeeded=ExecutionSucceeded(
                                                execution_id=execution_id,
                                                output_payload_json=output_payload_json,
                                                timestamp=datetime.now(UTC),
                                            ),
                                        )
                                    )
                                except Exception as e:
                                    await executor_messages.put(
                                        ExecutorMessage(
                                            execution_failed=ExecutionFailed(
                                                error_message="".join(
                                                    traceback.format_exception(
                                                        type(e), e, e.__traceback__
                                                    )
                                                ),
                                                execution_id=execution_id,
                                                timestamp=datetime.now(UTC),
                                            )
                                        )
                                    )

                            finally:
                                try:
                                    del state.running_executions[execution_id]
                                except KeyError:
                                    pass

                        state.running_executions[execution_id] = asyncio.create_task(
                            _run_execution()
                        )
                    case _, ExecutionCancelled(execution_id=execution_id):
                        task = state.running_executions.get(execution_id)
                        if task is not None:
                            task.cancel()

                        try:
                            del state.running_executions[execution_id]
                        except KeyError:
                            pass
                    case name, _:
                        logger.warning("unrecognized server message", name=name)
        finally:
            state.shutting_down = True
            for task in state.running_executions.values():
                task.cancel()


class _ExecutionHandler:
    def __init__(
        self,
        job_type: type[JobType[Any]],
        fn: Callable[[ExecutionContext, str], Awaitable[str]],
    ):
        self.fn = fn
        self.job_type = job_type


class _ExecutorState:
    def __init__(self):
        self.heartbeat_interval_seconds: float = 1.0
        self.executor_id: str | None = None
        self.shutting_down: bool = False
        self.running_executions: dict[str, asyncio.Task[Any]] = {}
