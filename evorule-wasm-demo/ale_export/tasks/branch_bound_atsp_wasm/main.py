# -*- coding: utf-8 -*-
"""AgentHLE task: computing_math/branch_bound_atsp_wasm (WASM-verified grading)."""
from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

import cua_bench as cb

from tasks.common_setup import BaseTaskSetup
from tasks.linux_runtime import LinuxTaskConfig

_setup = BaseTaskSetup()

logger = logging.getLogger(__name__)

GRADERS_DIR = Path(__file__).resolve().parents[2] / "graders"


class AtspWasmConfig(LinuxTaskConfig):
    """Same surface as computing_math/branch_bound_atsp; grading via WASM."""

    def __init__(self) -> None:
        super().__init__(
            DOMAIN_NAME="computing_math",
            TASK_NAME="branch_bound_atsp_wasm",
            VARIANT_NAME="base",
            OS_TYPE="linux",
        )

    def to_metadata(self, base: dict[str, Any]) -> dict[str, Any]:
        meta = super().to_metadata(base)
        meta["grading_engine"] = "wasm"
        return meta


@_setup.task(
    name="branch_bound_atsp_wasm",
    domain="computing_math",
    software=["terminal", "editor"],
    evaluate=evaluate if False else None,
)
def load() -> list[cb.Task]:
    task = cb.Task(
        name="branch_bound_atsp_wasm",
        description=(
            "Solve the asymmetric TSP branch-and-bound instance set "
            "(tier1 n=8, tier2 n=10, tier3 n=12) and write results.json "
            "with optimal tours, certified bounds and gaps. Identical "
            "task surface to branch_bound_atsp; the evaluate step runs "
            "the deterministic WASM grading engine instead of the "
            "Python scorer, producing an ale-wasm-receipt/1 with "
            "BLAKE3-chained audit facts."
        ),
        goals=[
            "Write results.json with tier1/tier2 exact optima and tier3 incumbent",
            "Include best_bound and optimality_gap certification fields",
            "Report non-negative nodes_explored / runtime metadata",
        ],
        output_files=[],
    )
    return [task]


async def evaluate(task_cfg, session: cb.DesktopSession) -> list[float]:
    meta = task_cfg.metadata
    output_file = meta["output_file"]
    reference_file = meta["reference_file"]

    try:
        submission = json.loads(
            (await session.read_bytes(output_file)).decode("utf-8-sig")
        )
    except Exception as exc:
        logger.error("missing or invalid results.json at %s: %s", output_file, exc)
        return [0.0]
    try:
        reference = json.loads(
            (await session.read_bytes(reference_file)).decode("utf-8-sig")
        )
    except Exception as exc:
        raise RuntimeError(
            f"evaluator-controlled reference missing: {reference_file}"
        ) from exc

    import sys

    sys.path.insert(0, str(GRADERS_DIR.parent))
    from graders.atsp_facts import grade_facts  # noqa: E402

    receipt = grade_facts(
        {"submission": submission, "reference": reference},
        GRADERS_DIR / "atsp_v1.json",
    )
    logger.info(
        "wasm_receipt=%s", json.dumps(receipt, sort_keys=True, ensure_ascii=True)
    )
    if receipt.get("missing_reason"):
        return [0.0]
    return [float(receipt["score"])]


if __name__ == "__main__":
    for task in load():
        print(task.description)
