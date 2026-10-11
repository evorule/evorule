# -*- coding: utf-8 -*-
"""ising_post_measurement bridge —— 官方 scorer 语义原样移植（判据面）。

产出 check-booleans（供 ising_v1.json 规则集组合，pass/fail 门控形态）：
  gate_required_outputs / gate_shapes_finite /
  pass_critical_state / pass_post_probs / pass_rdm_site1 / pass_correlators

官方语义来源：tasks/computing_math/ising_post_measurement_1/scripts/
score_outputs.py + variant_specs.py（rdi-berkeley/agents-last-exam）。
容差与重算全部在本层完成（浮点不进 wasm）；wasm 只组合布尔。
"""
from __future__ import annotations

import io
import json
import sys
from pathlib import Path
from typing import Any

import numpy as np

STATE_INFIDELITY_TOLERANCE = 1e-4
MAX_ABS_ERROR_TOLERANCE = 1e-2
REQUIRED_CORRELATOR_KEYS = ("Z_one_body", "X_one_body")

_parent = str(Path(__file__).resolve().parents[1])
if _parent not in __import__("sys").path:
    __import__("sys").path.insert(0, _parent)

from wasmfacts import grade_facts  # noqa: E402


def _variant(dim: int, requires_correlators: bool) -> dict[str, Any]:
    return {
        "expected_dimension": dim,
        "requires_correlators": requires_correlators,
        "required_outputs": ["critical_state.npy", "post_probs.npy", "rdm_site1.npy"]
        + (["correlators.npz"] if requires_correlators else []),
    }


def _load_npy(blob: bytes, *, label: str) -> np.ndarray:
    return np.load(io.BytesIO(blob), allow_pickle=False)


def _load_npz(blob: bytes, *, label: str) -> dict[str, np.ndarray]:
    with np.load(io.BytesIO(blob), allow_pickle=False) as archive:
        return {key: archive[key] for key in archive.files}


def _finite(arr: np.ndarray) -> bool:
    return bool(np.issubdtype(arr.dtype, np.number) and np.all(np.isfinite(arr)))


def _state_infidelity(agent: np.ndarray, reference: np.ndarray) -> float:
    agent_norm_sq = float(np.vdot(agent, agent).real)
    reference_norm_sq = float(np.vdot(reference, reference).real)
    if agent_norm_sq <= 0.0 or reference_norm_sq <= 0.0:
        return float("inf")
    fidelity = (abs(np.vdot(reference, agent)) ** 2) / (reference_norm_sq * agent_norm_sq)
    fidelity = min(max(float(fidelity), 0.0), 1.0)
    return 1.0 - fidelity


def _max_abs_error(agent: np.ndarray, reference: np.ndarray) -> float:
    return float(np.max(np.abs(agent - reference)))


def build_checks(agent_payloads: dict[str, bytes], reference_payloads: dict[str, bytes],
                 expected_dim: int, requires_correlators: bool) -> dict[str, int]:
    """官方 score_submission_bytes 语义。任何 ValueError→对应 pass_* 清零（官方 _fail）。"""
    checks: dict[str, int] = {"gate_required_outputs": 0, "gate_shapes_finite": 0}
    try:
        for name in _variant(expected_dim, requires_correlators)["required_outputs"]:
            if name not in agent_payloads or name not in reference_payloads:
                return checks
        checks["gate_required_outputs"] = 1

        # shapes+finite 前置（官方逐文件检查链，任一坏→0 分整体）
        ca = _load_npy(agent_payloads["critical_state.npy"], label="critical_state.npy")
        cr = _load_npy(reference_payloads["critical_state.npy"], label="ref")
        pa = _load_npy(agent_payloads["post_probs.npy"], label="post_probs.npy")
        pr = _load_npy(reference_payloads["post_probs.npy"], label="ref")
        ra = _load_npy(agent_payloads["rdm_site1.npy"], label="rdm_site1.npy")
        rr = _load_npy(reference_payloads["rdm_site1.npy"], label="ref")
        if not (_finite(ca) and _finite(cr) and _finite(pa) and _finite(pr) and _finite(ra) and _finite(rr)):
            return checks
        if ca.shape != (expected_dim,) or pa.shape != (expected_dim,) or ra.shape != (expected_dim, 2, 2):
            return checks
        if cr.shape != (expected_dim,) or pr.shape != (expected_dim,) or rr.shape != (expected_dim, 2, 2):
            return checks
        corr_a = corr_r = None
        if requires_correlators:
            corr_a = _load_npz(agent_payloads["correlators.npz"], label="correlators.npz")
            corr_r = _load_npz(reference_payloads["correlators.npz"], label="ref")
            expected_corr = (expected_dim,)  # shape 严格性见 correlators pass 段
            for key in REQUIRED_CORRELATOR_KEYS:
                if key not in corr_a or key not in corr_r:
                    return checks
                if not (_finite(corr_a[key]) and _finite(corr_r[key])):
                    return checks
        checks["gate_shapes_finite"] = 1

        checks["pass_critical_state"] = int(
            _state_infidelity(ca, cr) <= STATE_INFIDELITY_TOLERANCE
        )
        checks["pass_post_probs"] = int(
            _max_abs_error(pa, pr) <= MAX_ABS_ERROR_TOLERANCE
        )
        checks["pass_rdm_site1"] = int(
            _max_abs_error(ra, rr) <= MAX_ABS_ERROR_TOLERANCE
        )
        if requires_correlators:
            checks["pass_correlators"] = int(
                _max_abs_error(corr_a[REQUIRED_CORRELATOR_KEYS[0]], corr_r[REQUIRED_CORRELATOR_KEYS[0]]) <= MAX_ABS_ERROR_TOLERANCE
                and _max_abs_error(corr_a[REQUIRED_CORRELATOR_KEYS[1]], corr_r[REQUIRED_CORRELATOR_KEYS[1]]) <= MAX_ABS_ERROR_TOLERANCE
            )
        else:
            checks["pass_correlators"] = 1  # 不要求→不构成扣分（官方 skip）
        return checks
    except Exception:
        return {k: 0 for k in checks}  # 官方 ValueError→_fail(0.0) 同构


def grade_facts(facts: dict, ruleset_path: str | Path, engine: str | Path | None = None) -> dict:
    """facts = {agent_payloads, reference_payloads, expected_dim, requires_correlators}
    → wasm grade。instruction=eval_ising_v1（规则集定义的应用级指令）。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
    import wasmfacts
    payload = build_checks(
        facts["agent_payloads"], facts["reference_payloads"],
        facts["expected_dim"], facts["requires_correlators"])
    payload["task_id"] = facts.get("task_id", "computing_math/ising_post_measurement_wasm")
    return wasmfacts.engine_call(
        ruleset_path, payload, payload["task_id"],
        instruction={"type": "eval_ising_v1", "params": {"task_id": payload["task_id"]}},
    )


if __name__ == "__main__":
    print(__doc__)
