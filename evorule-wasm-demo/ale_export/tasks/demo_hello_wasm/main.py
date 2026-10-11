"""Demo task: ``demo/hello_wasm`` (Linux) — WASM-verified variant of demo/hello.

Identical task surface to the canonical ``demo/hello`` (same GUI flow, same
files), with ONE difference: the evaluate() decision core delegates to the
EvoRule WASM grading engine, producing an independently verifiable receipt
(BLAKE3 audit chain over the fact stream) alongside the official score.

Official framework contract is preserved end-to-end:
  * evaluate(task_cfg, session) -> list[float]  (unchanged signature)
  * session I/O stays official (read_file via DesktopSession)
  * strip() normalization stays in the bridge layer, receipt records it

License: Apache-2.0 — Contribution Grant GRANT-2026-ALE-001, EvoRule Project.
The wasm engine embeds EvoRule tcb/reactor/governance subsets (same grant).
"""
from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path

import cua_bench as cb

from tasks.linux_runtime import LinuxTaskConfig

logger = logging.getLogger(__name__)

DOMAIN_NAME = "demo"
TASK_NAME = "hello_wasm"
VARIANT_NAME = "base"

# bridge library ships with this task package (self-contained)
_WASMFACTS_DIR = Path(__file__).resolve().parent.parent.parent / "ale_export"
import sys  # noqa: E402

if str(_WASMFACTS_DIR) not in sys.path:
    sys.path.insert(0, str(_WASMFACTS_DIR))
import wasmfacts  # noqa: E402


@dataclass
class TaskConfig(LinuxTaskConfig):
    DOMAIN_NAME: str = DOMAIN_NAME
    TASK_NAME: str = TASK_NAME
    VARIANT_NAME: str = VARIANT_NAME

    @property
    def app_path(self) -> str:
        return f"{self.input_dir}/app.html"

    @property
    def order_path(self) -> str:
        return f"{self.input_dir}/order.json"

    @property
    def instructions_path(self) -> str:
        return f"{self.input_dir}/instructions.txt"

    @property
    def launcher_path(self) -> str:
        return f"{self.software_dir}/launch.sh"

    @property
    def result_path(self) -> str:
        return f"{self.remote_output_dir}/result.txt"

    @property
    def reference_result_path(self) -> str:
        return f"{self.reference_dir}/result.txt"

    @property
    def task_description(self) -> str:
        return (
            "Acme Code Generator — a GUI task. Use the desktop GUI (screenshot, "
            "click, type) to generate a confirmation code in a web app; the "
            "answer cannot be obtained from files alone.\n\n"
            f"1. Open the app: run `bash {self.launcher_path}` to launch Chrome "
            f"on it, or open {self.app_path} in Chrome yourself.\n"
            f"2. Read {self.order_path} to get the Order ID.\n"
            "3. In the app: click the \"Order ID\" field, type the Order ID, "
            "then click \"Generate Code\".\n"
            "4. The app displays a confirmation code (CONF-...). Read it from "
            "the screen.\n"
            f"5. Write that code, exactly, on a single line, to "
            f"{self.result_path}.\n\n"
            f"See {self.instructions_path} for the same steps. The code is "
            "computed by the app from the Order ID you type, so you must use "
            "the GUI."
        )

    def to_metadata(self) -> dict:
        m = super().to_metadata()
        m.update({
            "app_path": self.app_path,
            "order_path": self.order_path,
            "instructions_path": self.instructions_path,
            "launcher_path": self.launcher_path,
            "result_path": self.result_path,
            "reference_result_path": self.reference_result_path,
        })
        return m


@cb.tasks_config(split="train")
def load():
    cfg = TaskConfig()
    return [cb.Task(
        description=cfg.task_description,
        metadata=cfg.to_metadata(),
        computer={
            "provider": "computer",
            "setup_config": {"os_type": cfg.OS_TYPE},
        },
    )]


@cb.setup_task(split="train")
async def start(task_cfg, session: cb.DesktopSession):
    """Same as demo/hello: ensure clean output dir + staged input present."""
    meta = task_cfg.metadata
    out_dir = meta["remote_output_dir"]
    await session.run_command(f"mkdir -p {out_dir!r}", check=False)
    await session.run_command(f"rm -f {meta['result_path']!r}", check=False)

    for path in (meta["app_path"], meta["order_path"]):
        try:
            await session.read_file(path)
        except Exception as exc:
            raise RuntimeError(
                f"staged input missing: {path} unreadable ({exc}). "
                "Re-bake the image or check the GCS task data."
            )
    logger.info("[hello_wasm] staged input present; output dir ready at %s", out_dir)


@cb.evaluate_task(split="train")
async def evaluate(task_cfg, session: cb.DesktopSession) -> list[float]:
    """Exact-match via WASM engine — official score, verifiable receipt.

    Decision core delegated to wasmfacts.grade() (EvoRule WASM grader);
    everything else identical to the official demo/hello evaluate().
    """
    meta = task_cfg.metadata
    out_path = meta["result_path"]
    ref_path = meta["reference_result_path"]

    try:
        expected_raw = await session.read_file(ref_path)
    except Exception as exc:
        # Reference is staged (decrypted) by the framework before evaluate.
        raise RuntimeError(f"reference unreadable at {ref_path}: {exc}")

    try:
        actual_raw = await session.read_file(out_path)
    except Exception:
        actual_raw = None  # missing artifact → engine-side attribution

    facts = {"expected": expected_raw.strip()}
    if actual_raw is not None:
        facts["actual"] = actual_raw.strip()

    receipt = wasmfacts.grade_facts(facts, "demo/hello_wasm")
    logger.info(
        "[hello_wasm] score=%s chain_head=%s facts_hash=%s missing=%s",
        receipt["score"], receipt["audit_chain_head"][:16],
        receipt["facts_hash"][:16], receipt["missing_reason"],
    )
    return [float(receipt["score"])]
