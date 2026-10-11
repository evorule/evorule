"""AgentHLE task: computing_math/ising_post_measurement_wasm.

Same task surface as computing_math/ising_post_measurement_1 (critical
state, post-measurement probabilities, site-1 reduced density matrix,
optional correlators). The evaluate step delegates the decision core to
the deterministic WASM grading engine, producing an
ale-wasm-receipt/1 with BLAKE3-chained audit facts.
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
    def _identity(split=None, **_kw):  # accepts config_class etc.
        def _wrap(fn):
            return fn
        return _wrap
    def _task_decorator(fn):  # cb.Task(cls) identity
        return fn

    cb = _types.SimpleNamespace(  # type: ignore[assignment]
        Task=_task_decorator,
        DesktopSession=object,
        tasks_config=_identity,
        setup_task=_identity,
        evaluate_task=_identity,
    )

try:
    from tasks.common_setup import BaseTaskSetup
    from tasks.linux_runtime import LinuxTaskConfig
except ImportError:  # standalone (outside official harness)
    BaseTaskSetup = object
    LinuxTaskConfig = object

_setup = BaseTaskSetup()

GRADERS_DIR = Path(__file__).resolve().parents[2] / "graders"
if str(GRADERS_DIR) not in sys.path:
    sys.path.insert(0, str(GRADERS_DIR))

logger = logging.getLogger(__name__)

DOMAIN_NAME = "computing_math"
TASK_NAME = "ising_post_measurement_wasm"
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


class IsingPostMeasurementWasmConfig(LinuxTaskConfig):
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
    def data_dir_name(self) -> str:
        return "base"

    @property
    def description(self) -> str:
        return "Post-measurement Ising state reconstruction (WASM-verified grading)"


@cb.tasks_config(config_class=IsingPostMeasurementWasmConfig)
@cb.setup_task(_setup)
@cb.Task
class IsingPostMeasurementWasmTask:
    def __init__(self, main_config: IsingPostMeasurementWasmConfig) -> None:
        self._main_config = main_config

    def load_data(self) -> dict[str, Any]:
        return _setup.load_task_data(
            self._main_config.data_dir_name, self._main_config.TASK_NAME
        )

    @cb.evaluate_task
    def evaluate(self, task_input: Any, workspace: Any = None) -> dict[str, Any]:
        from ising_facts import grade_facts

        output_dir = Path(workspace) / self._main_config.output_dir_name if workspace else Path("output")

        def _read(name: str) -> bytes | None:
            p = output_dir / name
            return p.read_bytes() if p.exists() else None

        agent_payloads: dict[str, bytes] = {}
        for name in ("critical_state.npy", "post_probs.npy", "rdm_site1.npy", "correlators.npz"):
            blob = _read(name)
            if blob is not None:
                agent_payloads[name] = blob

        # Reference payloads (evaluator-side; mirrors official scorer packaging)
        reference_payloads = task_input.get("reference_payloads", {})
        expected_dim = int(task_input.get("expected_dim", 1 << 10))
        requires_correlators = bool(task_input.get("requires_correlators", False))

        missing = [n for n in ("critical_state.npy", "post_probs.npy", "rdm_site1.npy")
                   + (["correlators.npz"] if requires_correlators else [])
                   if n not in agent_payloads]

        facts = {
            "agent_payloads": agent_payloads,
            "reference_payloads": reference_payloads,
            "expected_dim": expected_dim,
            "requires_correlators": requires_correlators,
            "task_id": TASK_ID,
        }
        receipt = grade_facts(facts, GRADERS_DIR / "ising_v1.json")
        result = dict(receipt)
        result["missing_outputs"] = missing
        return result


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    logger.info("ising_post_measurement_wasm task module loaded (standalone)")
