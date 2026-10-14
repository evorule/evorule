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


def rule_entry(task: str, ruleset: list, source: str) -> dict:
    """Rule 条目（BundleEntry 同构，B-3 判卷轨）：rule_body=transform 指令集。

    15-B §1.1 双轨裁定：同质性校验禁 Rule/Knowledge 混装 → 判卷规则集
    须独立 Rule 包（entry_kind=rule）。rule_body=引擎原生 {transform:[...]}
    （server 侧 validate_rule_input 认 {transform:[]} 或裸数组；装载后进
    reactor，branch 匹配 instruction_type=eval_* → eq 比对 facts → set 万分制）。
    无 io 服务依赖 → 符号三方一致零要求；bundle 恒 L3 禁 enforce（ruleset 无
    enforce 指令，天然满足）。
    """
    return {
        "entry_id": f"eval.{task}.grader_rules",
        "entry_kind": "rule",
        "rule_body": {"transform": ruleset},
        "provenance": {"source": source},
        "domain": "eval",
        "tags": ["ale", "grader", "judging"],
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
        entry(task_dir, "task_card", "https://evorule.org/schemas/eval/task-card/v1.json",
              build_task_card_body(task_card, task_id),
              f"rdi-berkeley/agents-last-exam {task_id}"),
    ]
    ruleset = json.loads((graders_p / ruleset_name).read_text(encoding="utf-8"))
    ruleset_key = ruleset_name.removesuffix(".json")
    entries.append(entry(
        task_dir, "grader_ruleset", "https://evorule.org/schemas/eval/ruleset/v1.json",
        build_ruleset_body(ruleset, ruleset_key),
        "ale_export/graders/" + ruleset_name))
    if bridge_name is not None:
        entries.append(entry(
            task_dir, "grader_bridge", "https://evorule.org/schemas/eval/bridge/v1.json",
            build_bridge_body(graders_p / bridge_name, bridge_name),
            "ale_export/graders/" + bridge_name))
    variants = TASK_VARIANTS[task_dir]
    entries.append(entry(
        task_dir, "grader_test", "https://evorule.org/schemas/eval/test/v1.json",
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
        "bundle_schema_version": "1.0",
        "bundle_id": f"eval-task-{short}-v1",
        "dataset": {
            "dataset_id": f"eval-task-{short}",
            "name": f"ALE eval task {task_id}",
            "tenant_id": "eval",
            "instance_id": "evorule-ale-grif",
            "versioning": {"current": "v1", "chain": ["v1"]},
            # 供体轨单版本钉死（15-B §1.2 裁定）：pinned 完全锁定（include_patch=false），
            # 否则 rule 仓导入第 5 步走默认 auto_by_effective_date 分支缺生效基准 → MissingEffectiveBase 拒收
            "version_selection": {
                "mode": "pinned",
                "pinned_version": "v1",
                "pinned_include_patch": False,
            },
        },
        "entries": entries,
        "tests": {"subset": [v["name"] for v in variants], "verdict": "fail"},
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

    # ---- B-3 判卷轨：Rule 双导出（独立 Rule 包，同质性校验禁混装）----
    # dataset 元数据复用 Knowledge 包（同 dataset_id 声明治理谱系），bundle_id
    # 加 -rules 后缀区分；entries=单 Rule 条目（ruleset 原样 transform）。
    rule_bundle = {
        "bundle_schema_version": "1.0",
        "bundle_id": f"eval-task-{short}-v1-rules",
        "dataset": dict(bundle["dataset"]),
        "entries": [rule_entry(task_dir, ruleset, "ale_export/graders/" + ruleset_name)],
        "tests": {"subset": [evidence_tag(v["name"]) for v in variants], "verdict": "fail"},
        "audit": {
            "exported_at": "2026-10-12T00:00:00Z",
            "exported_by": "ale2bundle v0.2",
            "source_version": "v1",
            "content_hash": "",
            "hash_algo": "blake3",
        },
    }
    rule_bundle["audit"]["content_hash"] = dataset_bundle_hash(rule_bundle)
    rule_out = out_dir / "rules_bundle.json"
    rule_out.write_text(json.dumps(rule_bundle, ensure_ascii=False, indent=2), encoding="utf-8")
    return out


def evidence_tag(variant_name: str) -> str:
    """tests.subset 背书标记（server ①.5 测试证据校验）：O-431 后闸面收紧——
    verdict=pass 导入须 sandbox:<数字ID> 机器背书（human: 全链下线）。
    B-1/B-2 对拍账（03-M8 差测核心变体）发生在闸收紧前，本批按 unverified
    形态重生成（verdict=fail 起步，19 号档立法）；沙盒化验证（C1 面上线后
    补 sandbox 背书）登记为 B-4 验证面后续项。变体名存 subset 保可追溯。
    """
    return f"sandbox-pending/ale-porting/03-M8-diff-ledger/{variant_name}"


def main() -> int:
    ale_export = Path(__file__).resolve().parent.parent
    out_root = ale_export / "b_bundles"
    ok = 0
    for task_dir in ["demo_hello_wasm", "branch_bound_atsp_wasm",
                     "ising_post_measurement_wasm", "k8s_payment_api_root_cause_wasm"]:
        out = convert_task(ale_export, task_dir, out_root)
        # 自验：重新装载 + 哈希重算（两包各验）
        b = json.loads(out.read_text(encoding="utf-8"))
        recorded = b["audit"]["content_hash"]
        recomputed = dataset_bundle_hash(b)
        rb = json.loads((out.parent / "rules_bundle.json").read_text(encoding="utf-8"))
        r_recorded = rb["audit"]["content_hash"]
        r_recomputed = dataset_bundle_hash(rb)
        ok_k = recorded == recomputed
        ok_r = r_recorded == r_recomputed
        if ok_k and ok_r:
            ok += 1
        print(f"{'OK ' if ok_k else 'FAIL'} {out.parent.name}  blake3={recorded[7:23]}…")
        print(f"{'OK ' if ok_r else 'FAIL'} {out.parent.name}-rules  blake3={r_recorded[7:23]}…")
    print(f"\n{ok}/4 task dirs, dual-bundle hash self-verify {'PASS' if ok == 4 else 'FAIL'}")
    return 0 if ok == 4 else 1


if __name__ == "__main__":
    raise SystemExit(main())
