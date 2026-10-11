# -*- coding: utf-8 -*-
"""k8s_payment_api_root_cause bridge —— 官方 scorer 语义原样移植（判据面）。

产出 check-booleans 与整数组件值（供 k8s_v1.json 规则集组合）：
  硬门：gate_valid_json / gate_required_keys / gate_has_causes
  组件（万分制整数，由官方浮点权重×组件分×10000 四舍五入预归一）：
  comp_primary(3000) / comp_secondary(2000) / comp_tertiary(1000) /
  comp_resources(0..1000) / comp_remediation(0..1500) /
  comp_evidence(0..1000) / comp_summary(0..500)

官方语义来源：tasks/computing_math/k8s_payment_api_root_cause_analysis/
scripts/score_root_cause_analysis.py（rdi-berkeley/agents-last-exam）。
正则/集合运算全在本层；wasm 只做硬门+整数加法（score_fixed=Σcomp）。
浮点边界：官方 round(sum,6) 与万分制整数和的偏差≤7×10⁻⁵——
score_fixed 为整数和（万分位），float 展示=score_fixed/10000（诚实边界：
与官方 round6 相差<1e-4，不宣称逐位一致；对拍用 1e-3 容差）。
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

_parent = str(Path(__file__).resolve().parents[1])
if _parent not in sys.path:
    sys.path.insert(0, _parent)

REQUIRED_TOP_LEVEL = {
    "incident_metadata", "root_causes", "affected_resources",
    "remediation_plan", "summary",
}
REFERENCE_RESOURCES = {
    "deployment/payment-api@prod",
    "horizontalpodautoscaler/payment-api@prod",
    "pod/payment-api-7c9d4b85f5-2m8xq@prod",
    "pod/payment-api-7c9d4b85f5-kz7rv@prod",
}


def _norm_type(value: Any) -> str:
    return re.sub(r"[^a-z0-9]", "", str(value or "").lower())


def _text(value: Any) -> str:
    return str(value or "")


def _root_causes(report: dict) -> list[dict]:
    values = report.get("root_causes")
    return [v for v in values if isinstance(v, dict)] if isinstance(values, list) else []


def _evidence_list(cause: dict) -> list[str]:
    values = cause.get("evidence")
    if not isinstance(values, list):
        return []
    return [v for v in values if isinstance(v, str) and v.strip()]


def comp_primary(causes: list[dict]) -> int:
    tokens = ("oomkill", "oom", "memorylimit")
    for cause in causes:
        t = _norm_type(cause.get("type"))
        if any(tok in t for tok in tokens) and _text(cause.get("severity")).lower() == "critical":
            return 3000
    return 0


def comp_secondary(causes: list[dict]) -> int:
    for cause in causes:
        t = _norm_type(cause.get("type"))
        if not any(tok in t for tok in ("liveness", "probe", "startupprobe")):
            continue
        evidence = "\n".join(_evidence_list(cause))
        has_delay = "initialDelaySeconds: 5" in evidence
        has_warmup = ("duration=6.67s" in evidence or "duration=6.54s" in evidence
                      or "cache warm-up complete" in evidence)
        if has_delay and has_warmup:
            return 2000
    return 0


def comp_tertiary(causes: list[dict]) -> int:
    for cause in causes:
        haystack = " ".join([_text(cause.get("type")), _text(cause.get("description")),
                             " ".join(_evidence_list(cause))]).lower()
        if "metrics_port" not in haystack and "metrics port" not in haystack:
            continue
        if _text(cause.get("severity")).lower() in ("low", "medium"):
            return 1000
    return 0


def _normalize_resource(value: Any) -> str | None:
    if isinstance(value, dict):
        kind = _text(value.get("kind")).strip()
        name = _text(value.get("name")).strip()
        namespace = _text(value.get("namespace")).strip() or "prod"
        if kind and name:
            return f"{kind}/{name}@{namespace}".lower()
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None
        if "@" in text:
            return text.lower()
        for part in text.split():
            if "/" in part:
                return f"{part}@prod".lower()
    return None


def comp_resources(report: dict) -> int:
    values = report.get("affected_resources")
    observed: set[str] = set()
    if isinstance(values, list):
        for v in values:
            n = _normalize_resource(v)
            if n:
                observed.add(n)
    tp = len(observed & REFERENCE_RESOURCES)
    precision = tp / len(observed) if observed else 0.0
    recall = tp / len(REFERENCE_RESOURCES)
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return round(f1 * 1000)


def comp_remediation(report: dict) -> int:
    values = report.get("remediation_plan")
    action_text = ""
    if isinstance(values, list):
        action_text = "\n".join(
            _text(i.get("action") if isinstance(i, dict) else i) for i in values)
    patterns = {
        "memory_limit": r"memory.*(512|768|1Gi|1024)",
        "probe_relaxation": r"(initialDelaySeconds.*(20|[3-9][0-9])|startupProbe|startup probe)",
        "metrics_port": r"(METRICS_PORT|metrics port)",
    }
    matches = sum(bool(re.search(p, action_text, flags=re.IGNORECASE | re.DOTALL))
                  for p in patterns.values())
    return round(matches / 3 * 1500)


def comp_evidence(causes: list[dict], input_texts: list[str]) -> int:
    if not causes:
        return 0
    grounded = 0
    for cause in causes:
        evidence = _evidence_list(cause)
        if not evidence:
            continue
        if any(item not in "".join(input_texts) and not any(item in t for t in input_texts)
               for item in evidence):
            continue
        grounded += 1
    return round(grounded / len(causes) * 1000)


def comp_summary(report: dict) -> int:
    """官方 (0.5+0.5)×权重0.05 → 满分 500 万分制。"""
    summary_ok = isinstance(report.get("summary"), str) and bool(report["summary"].strip())
    metadata = report.get("incident_metadata")
    rollback = metadata.get("safe_to_rollback") if isinstance(metadata, dict) else None
    rollback_ok = rollback is True
    return (250 if summary_ok else 0) + (250 if rollback_ok else 0)


def build_facts(agent_report: str | bytes, input_texts: list[str]) -> dict[str, int]:
    """官方 score_report 语义。facts 键=comp_*（万分制整数）+三硬门。"""
    facts: dict[str, int] = {
        "gate_valid_json": 0, "gate_required_keys": 0, "gate_has_causes": 0,
        "comp_primary": 0, "comp_secondary": 0, "comp_tertiary": 0,
        "comp_resources": 0, "comp_remediation": 0, "comp_evidence": 0, "comp_summary": 0,
    }
    try:
        text = agent_report.decode("utf-8-sig") if isinstance(agent_report, bytes) else agent_report
        report = json.loads(text)
        if not isinstance(report, dict):
            return facts
    except Exception:
        return facts
    facts["gate_valid_json"] = 1
    if REQUIRED_TOP_LEVEL - set(report):
        return facts
    facts["gate_required_keys"] = 1
    causes = _root_causes(report)
    if not causes:
        return facts
    facts["gate_has_causes"] = 1
    facts["comp_primary"] = comp_primary(causes)
    facts["comp_secondary"] = comp_secondary(causes)
    facts["comp_tertiary"] = comp_tertiary(causes)
    facts["comp_resources"] = comp_resources(report)
    facts["comp_remediation"] = comp_remediation(report)
    facts["comp_evidence"] = comp_evidence(causes, input_texts)
    facts["comp_summary"] = comp_summary(report)
    return facts


def grade_facts(facts: dict, ruleset_path: str | Path, engine: str | Path | None = None) -> dict:
    """facts = {agent_report, input_texts} → wasm grade（eval_k8s_v1）。"""
    import wasmfacts
    payload = build_facts(facts["agent_report"], facts["input_texts"])
    payload["task_id"] = facts.get("task_id", "computing_math/k8s_payment_api_root_cause_wasm")
    return wasmfacts.engine_call(
        ruleset_path, payload, payload["task_id"],
        instruction={"type": "eval_k8s_v1", "params": {"task_id": payload["task_id"]}},
    )


if __name__ == "__main__":
    print(__doc__)
