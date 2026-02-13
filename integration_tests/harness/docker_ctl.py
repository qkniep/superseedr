from __future__ import annotations

import os
import subprocess
from pathlib import Path
from typing import Iterable


class DockerCompose:
    def __init__(self, compose_file: Path, project_name: str, env: dict[str, str]) -> None:
        self.compose_file = compose_file
        self.project_name = project_name
        self.env = {**os.environ, **env}

    def _cmd(self, args: Iterable[str]) -> list[str]:
        return [
            "docker",
            "compose",
            "-f",
            str(self.compose_file),
            "-p",
            self.project_name,
            *args,
        ]

    def run(self, args: Iterable[str], check: bool = True, capture: bool = False) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            self._cmd(args),
            env=self.env,
            check=check,
            text=True,
            capture_output=capture,
        )

    def up(self, services: list[str], no_build: bool = False) -> None:
        args = ["up", "-d"]
        if no_build:
            args.append("--no-build")
        args.extend(services)
        self.run(args)

    def down(self) -> None:
        self.run(["down", "-v", "--remove-orphans"], check=False)

    def ps(self) -> str:
        result = self.run(["ps"], check=False, capture=True)
        return result.stdout

    def logs(self, service: str, tail: int = 200) -> str:
        result = self.run(["logs", "--no-color", "--tail", str(tail), service], check=False, capture=True)
        return result.stdout

    def exec(self, service: str, command: list[str], check: bool = True, capture: bool = False) -> subprocess.CompletedProcess[str]:
        return self.run(["exec", "-T", service, *command], check=check, capture=capture)
