"""AgentHLE task: computing_math/branch_bound_atsp_wasm.

Same task surface as computing_math/branch_bound_atsp (tier1/2 exact
optima, tier3 incumbent; results.json with certified bounds and gaps).
The evaluate step delegates the decision core to the deterministic WASM
grading engine, producing an ale-wasm-receipt/1 with BLAKE3-chained
audit facts.
"""
from __future__ import annotations

import json
import logging
import os
import sys
from pathlib import Path
from typing import Any

try:
    import cua_bench as cb
except ImportError:  # standalone import (receipt replay / CI smoke)
    import types as _types

    _Task = _types.SimpleNamespace
    _identity = lambda split=None: (lambda fn: fn)  # noqa: E731
    cb = _types.SimpleNamespace(  # type: ignore[assignment]
        Task=_Task,
        DesktopSession=object,
        tasks_config=_identity,
        setup_task=_identity,
        evaluate_task=_identity,
    )

from tasks.common_setup import BaseTaskSetup
from tasks.linux_runtime import LinuxTaskConfig

_setup = BaseTaskSetup()

GRADERS_DIR = Path(__file__).resolve().parents[2] / "graders"
if str(GRADERS_DIR) not in sys.path:
    sys.path.insert(0, str(GRADERS_DIR))

logger = logging.getLogger(__name__)

DOMAIN_NAME = "computing_math"
TASK_NAME = "branch_bound_atsp_wasm"
TASK_ID = f"{DOMAIN_NAME}/{TASK_NAME}"
VARIANT_NAME = "base"
ALLOWED_OUTPUT_DIR_NAMES = {"output"}
ADMIN_OUTPUT_PREFIX = "output_admin_"


def _normalize_output_dir_name(raw: str) -> str:
    normalized = raw.replace("\\", "/").strip("/")
    if not normalized or "/" in normalized:
        raise ValueError(f"REMOTE_OUTPUT_DIR must be a single directory name, got {raw!r}")
    if normalized in ALLOWED_OUTPUT_DIR_NAMES or normalized.startswith(ADMIN_OUTPUT_PREFIX):
        return normalized
    raise ValueError(
        "REMOTE_OUTPUT_DIR must be one of "
        f"{sorted(ALLOWED_OUTPUT_DIR_NAMES)} or start with {ADMIN_OUTPUT_PREFIX!r}"
    )


class BranchBoundAtspWasmConfig(LinuxTaskConfig):
    DOMAIN_NAME: str = DOMAIN_NAME
    TASK_NAME: str = TASK_NAME
    VARIANT_NAME: str = VARIANT_NAME
    OS_TYPE: str = "linux"

    def __init__(self, remote_output_dir: str | None = None) -> None:
        self.REMOTE_OUTPUT_DIR = remote_output_dir or os.environ.get("REMOTE_OUTPUT_DIR", "output")
        self.DOMAIN_NAME = DOMAIN_NAME
        self.TASK_NAME = TASK_NAME
        self.VARIANT_NAME = VARIANT_NAME
        self.OS_TYPE = "linux"
        self.REQUIRES_TASK_DATA = True

    @property
    def output_dir_name(self) -> str:
        return _normalize_output_dir_name(self.REMOTE_OUTPUT_DIR)

    @property
    def input_dir(self) -> str:
        return f"{self.task_dir}/input"

    @property
    def reference_dir(self) -> str:
        return f"{self.task_dir}/reference"

    @property
    def software_dir(self) -> str:
        return f"{self.task_dir}/software"

    @property
    def remote_output_dir(self) -> str:
        return f"{self.task_dir}/{self.output_dir_name}"

    @property
    def problem_spec_file(self) -> str:
        return f"{self.input_dir}/problem_spec.md"

    @property
    def task_prompt_file(self) -> str:
        return f"{self.input_dir}/TASK_PROMPT.md"

    @property
    def output_file(self) -> str:
        return f"{self.remote_output_dir}/results.json"

    @property
    def reference_file(self) -> str:
        return f"{self.reference_dir}/results.json"

    @property
    def task_description(self) -> str:
        return f"""You are working on a Linux VM.

## Task Directory
`{self.task_dir}`

## Objective
Solve the asymmetric TSP branch-and-bound instance set and write
`{self.output_file}` with tier1/tier2 exact optima and a tier3
incumbent, each with certified bounds and gaps.

## Grading
The evaluate step runs the deterministic WASM grading engine: the
official scorer semantics run in the bridge layer (auditable
check-booleans); the WASM ruleset is the scoring SSOT. The receipt
(schema ale-wasm-receipt/1) is recomputable by anyone with a browser
or a zero-dependency CLI.
"""


config = BranchBoundAtspWasmConfig()


@cb.tasks_config(split="train")
def load():
    cfg = BranchBoundAtspWasmConfig()
    return [
        cb.Task(
            description=cfg.task_description,
            metadata=cfg.to_metadata(),
            computer={"provider": "computer", "setup_config": {"os_type": cfg.OS_TYPE}},
        )
    ]


@cb.setup_task(split="train")
async def start(task_cfg, session: cb.DesktopSession):
    await _setup(task_cfg, session)


@cb.evaluate_task(split="train")
async def evaluate(task_cfg, session: cb.DesktopSession) -> list[float]:
    meta = task_cfg.metadata
    if not (await session.file_exists(meta["output_file"]) or await session.directory_exists(meta["output_file"])):
        logger.error("missing results.json at %s", meta["output_file"])
        return [0.0]
    if not (await session.file_exists(meta["reference_file"]) or await session.directory_exists(meta["reference_file"])):
        raise RuntimeError(f"evaluator-controlled reference missing: {meta['reference_file']}")

    submission = json.loads((await session.read_bytes(meta["output_file"])).decode("utf-8-sig"))
    reference = json.loads((await session.read_bytes(meta["reference_file"])).decode("utf-8-sig"))

    from atsp_facts import grade_facts

    receipt = grade_facts(
        {"submission": submission, "reference": reference},
        GRADERS_DIR / "atsp_v1.json",
    )
    logger.info("wasm_receipt=%s", json.dumps(receipt, sort_keys=True, ensure_ascii=True))
    if receipt.get("missing_reason"):
        return [0.0]
    return [float(receipt["score"])]


if __name__ == "__main__":
    for task in load():
        print(task.description)
