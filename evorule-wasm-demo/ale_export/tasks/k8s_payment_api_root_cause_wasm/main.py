"""AgentHLE task: computing_math/k8s_payment_api_root_cause_wasm.

Same task surface as computing_math/k8s_payment_api_root_cause_analysis
(produce root_cause_report.json from cluster incident artifacts). The
evaluate step delegates the decision core to the deterministic WASM
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
TASK_NAME = "k8s_payment_api_root_cause_wasm"
TASK_ID = f"{DOMAIN_NAME}/{TASK_NAME}"
VARIANT_NAME = "base"
ALLOWED_OUTPUT_DIR_NAMES = {"output"}
ADMIN_OUTPUT_PREFIX = "output_admin_"
REPORT_NAME = "root_cause_report.json"


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


class K8sPaymentApiRootCauseWasmConfig(LinuxTaskConfig):
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
        return "K8s payment API root cause analysis (WASM-verified grading)"


@cb.tasks_config(config_class=K8sPaymentApiRootCauseWasmConfig)
@cb.setup_task(_setup)
@cb.Task
class K8sPaymentApiRootCauseWasmTask:
    def __init__(self, main_config: K8sPaymentApiRootCauseWasmConfig) -> None:
        self._main_config = main_config

    def load_data(self) -> dict[str, Any]:
        return _setup.load_task_data(
            self._main_config.data_dir_name, self._main_config.TASK_NAME
        )

    @cb.evaluate_task
    def evaluate(self, task_input: Any, workspace: Any = None) -> dict[str, Any]:
        from k8s_facts import grade_facts

        output_dir = Path(workspace) / self._main_config.output_dir_name if workspace else Path("output")
        report_path = output_dir / REPORT_NAME
        agent_report = report_path.read_text(encoding="utf-8") if report_path.exists() else ""

        input_texts = [
            str(task_input.get(k, "")) for k in
            ("cluster_state", "deployment_yaml", "failing_pod_log")
        ]

        missing = [] if report_path.exists() and agent_report.strip() else [REPORT_NAME]

        facts = {
            "agent_report": agent_report,
            "input_texts": input_texts,
            "task_id": TASK_ID,
        }
        receipt = grade_facts(facts, GRADERS_DIR / "k8s_v1.json")
        result = dict(receipt)
        result["missing_outputs"] = missing
        return result


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    logger.info("k8s_payment_api_root_cause_wasm task module loaded (standalone)")
