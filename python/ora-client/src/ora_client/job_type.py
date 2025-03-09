import inspect
import json
from datetime import UTC, datetime
from typing import Any

from pydantic import BaseModel, TypeAdapter

from ora_client.job_definition import JobDefinition


class JobType[OutputType](BaseModel):
    """
    A job type that can be executed by executors.

    Classes that inherit from this class
    are also used to input types for the job.
    """

    def __init_subclass__(cls, **kwargs: Any):
        super().__init_subclass__(**kwargs)

        generic_args = cls.__pydantic_generic_metadata__.get("args", [])

        if len(generic_args) < 1:
            setattr(cls, "__ora_output_type__", None)
        else:
            setattr(cls, "__ora_output_type__", generic_args[0])

    @classmethod
    def __ora_input_schema_json__(cls) -> str:
        """
        Return the JSON schema for the input of this job type.
        """
        return json.dumps(cls.model_json_schema(by_alias=True))

    @classmethod
    def __ora_output_schema_json__(cls) -> str:
        """
        Return the JSON schema for the output of this job type.
        """
        output_type = getattr(cls, "__ora_output_type__", None)

        if output_type is None:
            return '{"type": "null"}'

        if issubclass(output_type, BaseModel):
            return json.dumps(output_type.model_json_schema(by_alias=True))

        adapter = TypeAdapter(output_type)

        return json.dumps(adapter.json_schema(by_alias=True))

    @classmethod
    def __ora_job_type_id__(cls) -> str:
        """
        Return the ID of this job type.
        """
        return cls.__name__

    @classmethod
    def __ora_job_type_description__(cls) -> str:
        """
        Return the description of this job type.
        """

        return inspect.getdoc(cls) or ""

    def job(self) -> JobDefinition:
        """
        Create a job definition from this job type instance.
        """
        return JobDefinition(
            input_payload_json=self.model_dump_json(by_alias=True),
            job_type_id=self.__ora_job_type_id__(),
            target_execution_time=datetime.now(UTC),
        )
