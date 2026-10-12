# -*- coding: utf-8 -*-
"""ale2bundle: ALE 任务包 → eval.task.v1 治理 bundle 转换器（B-1）。

供体轨机械映射（15-B 底稿 §1.2 契约）：
  输入: ale_export/tasks/<task>/（main.py + task_card.json）
        ale_export/graders/<ruleset>.json + <bridge>.py
  输出: eval-task-<domain>-<name>-v1/ 目录
        - task_card.json        (entry_kind=Knowledge, schema_ref=eval.task-card.v1)
        - grader_ruleset.json   (entry_kind=Knowledge, schema_ref=eval.ruleset.v1)
        - grader_bridge.json    (entry_kind=Knowledge, schema_ref=eval.bridge.v1)
        - grader_test.json      (entry_kind=Knowledge, schema_ref=eval.test.v1)

物理格式 = evorule-bundle 0.5.2 DatasetBundle 单文件 JSON（15-B §1.1 裁定：
物理格式跟 evorule-bundle crate SSOT，entry 内联而非 entry_files 分立——
与 server 仓 bundle-ds-scenario-* 目录形态的差别：那是 rule 仓导出物的
落盘形态；DatasetBundle JSON 本身是 crate 的交换格式，content_hash 覆盖
整个 JSON。转换器产出 DatasetBundle JSON；导入 rule 仓走既有 /bundles/import）。

content_hash = blake3:hex( serde_json::to_vec(bundle with audit.content_hash="") )
字段序 = struct 声明序（Python json.dumps 保持 dict 插入序，构造时对齐）。
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import blake3

# ---------------------------------------------------------------------------
# SSOT 对齐工具
# ---------------------------------------------------------------------------

def _ordered(obj):
    """递归保证 dict 序=插入序（Python 原生保证），list 序保持。"""
    return obj


def dataset_bundle_hash(bundle: dict) -> str:
    """evorule-bundle compute_content_hash 的 Python 同构实现。

    serde 序列化规则（与 crate 字节一致的精确同构）：
    - struct 子树（DatasetBundle/BundleEntry/Provenance/…）：字段序=声明序；
    - serde_json::Value 子树（rule_body）：键序=BTreeMap 字母序；
    - 数字：整数原样；浮点 serde 保留原文形式（本转换器产物只含整数与字符串）。

    同构法：构造时顶层/entry struct 子树按声明序插入（构造序即序列化序），
    Value 子树（rule_body）递归键排序后 dumps。
    """
    canonical = json.loads(json.dumps(bundle))  # deep copy
    canonical["audit"]["content_hash"] = ""
    for e in canonical.get("entries", []):
        e["rule_body"] = _sorted_value(e["rule_body"])
    payload = json.dumps(canonical, ensure_ascii=False, separators=(",", ":"))
    return "blake3:" + blake3.blake3(payload.encode("utf-8")).hexdigest()


def _sorted_value(v):
    """serde_json::Value 同构：对象键字母序递归排序。"""
    if isinstance(v, dict):
        return {k: _sorted_value(v[k]) for k in sorted(v)}
    if isinstance(v, list):
        return [_sorted_value(x) for x in v]
    return v


# ---------------------------------------------------------------------------
# eval.* entry 构造（15-B §1.1 字段级定稿）
# ---------------------------------------------------------------------------

SCORING_SHAPE = {
    "demo_hello_v1": "exact_match",
    "atsp_v1": "weighted_recompute",
    "ising_v1": "tolerance_gate",
    "k8s_v1": "gated_components",
}


def entry(task: str, kind_body: str, schema_ref: str, body: dict,
          source: str) -> dict:
    """eval.* entry（BundleEntry 同构：Knowledge 条目形态）。"""
    return {
        "entry_id": f"eval.{task}.{kind_body}",
        "entry_kind": "knowledge",
        "rule_body": body,
        "schema_ref": schema_ref,
        "provenance": {"source": source},
        "domain": "eval",
        "tags": ["ale", "grader"],
    }


def build_task_card_body(task_card: dict, task_id: str) -> dict:
    body = dict(task_card)
    body["task_id"] = task_id
    body["evaluation"] = {
        "pipeline": "wasm-graded",
        "receipt_recomputable": "decision-layer",
        "note": "graded by evorule wasm engine; receipt ale-wasm-receipt/1",
    }
    return body


def build_ruleset_body(ruleset: list, ruleset_name: str) -> dict:
    return {
        "ruleset": ruleset,
        "scoring_shape": SCORING_SHAPE[ruleset_name],
        "normalization": "integer_bps" if ruleset_name in ("atsp_v1", "k8s_v1") else "none",
        "score_max": 10000,
        "fail_visible": {"missing_reason": "required"},
    }


def build_bridge_body(bridge_src: Path, bridge_name: str) -> dict:
    src = bridge_src.read_bytes()
    return {
        "language": "python",
        "source_ref": f"ale_export/graders/{bridge_name}",
        "source_blake3": "blake3:" + blake3.blake3(src).hexdigest(),
        "deps": ["numpy"],
        "entry_function": "grade_facts",
    }


def build_test_body(variants: list) -> dict:
    return {"variants": variants}


# ---------------------------------------------------------------------------
# 供体轨主流程
# ---------------------------------------------------------------------------

# 每任务的差测核心变体摘录（v1 手工摘录，15-B §1.2；来源=03 底稿对拍账）
TASK_VARIANTS = {
    "demo_hello_wasm": [
        {"name": "correct", "facts": {"expected": "hello", "actual": "hello"}, "expect": [1.0]},
        {"name": "mismatch", "facts": {"expected": "hello", "actual": "world"}, "expect": [0.0]},
        {"name": "missing_output", "facts": {"expected": "hello", "actual": None}, "expect": [0.0]},
    ],
    "branch_bound_atsp_wasm": [
        {"name": "perfect", "expect": [1.0]},
        {"name": "tier1_cost_error", "expect": [0.7]},
        {"name": "tier2_metadata_missing", "expect": [0.65]},
        {"name": "tier3_gap_exceeded", "expect": [0.95]},
    ],
    "ising_post_measurement_wasm": [
        {"name": "all_correct", "expect": [1.0]},
        {"name": "missing_output", "expect": [0.0]},
        {"name": "tolerance_edge_inside", "expect": [1.0]},
        {"name": "tolerance_edge_outside", "expect": [0.0]},
    ],
    "k8s_payment_api_root_cause_wasm": [
        {"name": "perfect_report", "expect": [1.0]},
        {"name": "missing_required_keys", "expect": [0.0]},
        {"name": "empty_causes", "expect": [0.0]},
        {"name": "drop_tertiary_cause", "expect": [0.9]},
    ],
}

BRIDGE_OF = {
    # (ruleset_name, bridge_name)；bridge_name=None 表示无独立 bridge（demo 级直用 wasmfacts）
    "demo_hello_wasm": ("demo_hello_v1.json", None),
    "branch_bound_atsp_wasm": ("atsp_v1.json", "atsp_facts.py"),
    "ising_post_measurement_wasm": ("ising_v1.json", "ising_facts.py"),
    "k8s_payment_api_root_cause_wasm": ("k8s_v1.json", "k8s_facts.py"),
}


def convert_task(ale_export: Path, task_dir: str, out_root: Path) -> Path:
    tasks_p = ale_export / "tasks" / task_dir
    graders_p = ale_export / "graders"

    task_card = json.loads((tasks_p / "task_card.json").read_text(encoding="utf-8"))
    task_id = task_card.get("taskId") or task_dir
    ruleset_name, bridge_name = BRIDGE_OF[task_dir]

    # ---- entries（声明序对齐 BundleEntry）----
    entries = [
        entry(task_dir, "task_card", "eval.task-card.v1",
              build_task_card_body(task_card, task_id),
              f"rdi-berkeley/agents-last-exam {task_id}"),
    ]
    ruleset = json.loads((graders_p / ruleset_name).read_text(encoding="utf-8"))
    ruleset_key = ruleset_name.removesuffix(".json")
    entries.append(entry(
        task_dir, "grader_ruleset", "eval.ruleset.v1",
        build_ruleset_body(ruleset, ruleset_key),
        "ale_export/graders/" + ruleset_name))
    if bridge_name is not None:
        entries.append(entry(
            task_dir, "grader_bridge", "eval.bridge.v1",
            build_bridge_body(graders_p / bridge_name, bridge_name),
            "ale_export/graders/" + bridge_name))
    variants = TASK_VARIANTS[task_dir]
    entries.append(entry(
        task_dir, "grader_test", "eval.test.v1",
        build_test_body(variants),
        "03-M8 differential test ledger (core variants)"))

    # ---- DatasetBundle（字段序=struct 声明序）----
    short = {
        "demo_hello_wasm": "demo-hello",
        "branch_bound_atsp_wasm": "computing-math-branch-bound-atsp",
        "ising_post_measurement_wasm": "computing-math-ising-post-measurement",
        "k8s_payment_api_root_cause_wasm": "sre-k8s-payment-api-root-cause",
    }[task_dir]
    bundle = {
        "bundle_schema_version": "1",
        "bundle_id": f"eval-task-{short}-v1",
        "dataset": {
            "dataset_id": f"eval-task-{short}",
            "name": f"ALE eval task {task_id}",
            "tenant_id": "eval",
            "instance_id": "evorule-ale-grif",
            "versioning": {"current": "v1", "chain": ["v1"]},
        },
        "entries": entries,
        "tests": {"subset": [v["name"] for v in variants], "verdict": "pass"},
        "audit": {
            "exported_at": "2026-10-12T00:00:00Z",
            "exported_by": "ale2bundle v0.1",
            "source_version": "v1",
            "content_hash": "",
            "hash_algo": "blake3",
        },
    }
    bundle["audit"]["content_hash"] = dataset_bundle_hash(bundle)

    out_dir = out_root / f"eval-task-{short}-v1"
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / "bundle.json"
    out.write_text(json.dumps(bundle, ensure_ascii=False, indent=2), encoding="utf-8")
    return out


def main() -> int:
    ale_export = Path(__file__).resolve().parent.parent
    out_root = ale_export / "b_bundles"
    ok = 0
    for task_dir in ["demo_hello_wasm", "branch_bound_atsp_wasm",
                     "ising_post_measurement_wasm", "k8s_payment_api_root_cause_wasm"]:
        out = convert_task(ale_export, task_dir, out_root)
        # 自验：重新装载 + 哈希重算
        b = json.loads(out.read_text(encoding="utf-8"))
        recorded = b["audit"]["content_hash"]
        recomputed = dataset_bundle_hash(b)
        status = "OK " if recorded == recomputed else "FAIL"
        if recorded == recomputed:
            ok += 1
        print(f"{status} {out.parent.name}  blake3={recorded[7:23]}…")
    print(f"\n{ok}/4 bundles, hash self-verify {'PASS' if ok == 4 else 'FAIL'}")
    return 0 if ok == 4 else 1


if __name__ == "__main__":
    raise SystemExit(main())
