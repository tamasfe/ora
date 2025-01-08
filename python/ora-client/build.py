import importlib.resources
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any, final

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


@final
class CustomBuildHook(BuildHookInterface):
    def initialize(self, version: str, build_data: dict[str, Any]) -> None:
        super().initialize(version, build_data)

        if self.target_name == "sdist":
            generate_proto()


def generate_proto():
    print("generating protobuf bindings...", flush=True)
    project_root = Path(os.path.dirname(__file__))

    proto_definitions_root = (project_root / ".." / ".." / "proto").resolve()
    package_root = project_root / "src" / "ora_client"
    proto_out_root = package_root / "proto"

    well_known_proto_definitions_root = Path(
        str(importlib.resources.files("grpc_tools").joinpath("_proto"))
    )

    shutil.rmtree(proto_out_root, ignore_errors=True)
    os.makedirs(proto_out_root, exist_ok=True)

    proto_file_paths: list[Path] = []

    # Include well-known types in the descriptor set
    for dir_path, _, file_names in os.walk(well_known_proto_definitions_root):
        if dir_path.endswith("compiler"):
            continue

        for file_name in file_names:
            if file_name.endswith(".proto"):
                proto_file = Path(dir_path) / file_name
                proto_file_paths.append(proto_file)

    for dir_path, _, file_names in os.walk(proto_definitions_root):
        for file_name in file_names:
            if file_name.endswith(".proto"):
                proto_file = Path(dir_path) / file_name
                proto_file_paths.append(proto_file)

    protoc_args = [
        f"-I{proto_definitions_root}",
        f"--python_betterproto_out={proto_out_root}",
        f"--descriptor_set_out={proto_out_root / 'descriptor_set.bin'}",
        *(str(proto_file) for proto_file in proto_file_paths),
    ]

    ret = subprocess.run(
        [
            "python3",
            "-W",
            "ignore::DeprecationWarning",
            "-m",
            "grpc_tools.protoc",
            *protoc_args,
        ],
        capture_output=True,
        check=True,
    ).returncode

    if ret != 0:
        raise RuntimeError("Failed to generate protobuf code")
