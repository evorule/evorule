#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 EvoRule Project
# This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
"""
agents-md-to-schema.py — 从 AGENTS.md 抽取结构化信息,验证 AGENTS.schema.json 同步状态

设计原则 (走神 6 校准):
  - 人类写 narrative 时是 markdown (AGENTS.md 是 source of truth)
  - LLM/agent 读结构化时是 JSON (AGENTS.schema.json 是 consumer format)
  - 这个脚本不做"自动生成",做"diff reporter":
    改了 AGENTS.md 后跑脚本,看哪些章节需要更新到 schema.json

用法:
  python scripts/agents-md-to-schema.py           # diff 模式 (默认)
  python scripts/agents-md-to-schema.py --check   # CI 模式 (有 diff 退出码 1)
  python scripts/agents-md-to-schema.py --extract # 只抽取并打印 JSON
  python scripts/agents-md-to-schema.py --validate # 只校验 schema.json 合法性
"""
import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).parent.parent
# AGENTS.md / AGENTS.schema.json 已移至 L2 仓内（文档/，.gitignore 保护不发布）
# L1 不再有 agent 工作规则与机器可读附录；此脚本仅用于 L2 内部双轨制校验
AGENTS_MD = REPO_ROOT / "文档" / "AGENTS.md"
SCHEMA_JSON = REPO_ROOT / "文档" / "AGENTS.schema.json"


def extract_accepted_rules(md: str) -> list[str]:
    """提取 ✅ 接受 section 下的所有条目 (L56-65)"""
    section = re.search(
        r"###\s*✅\s*接受.*?(?=###\s*❌|###\s*边界判断|\Z)",
        md, re.DOTALL
    )
    if not section:
        return []
    items = re.findall(r"^-\s+(.+?)$", section.group(0), re.MULTILINE)
    return [item.strip() for item in items]


def extract_rejected_rules(md: str) -> list[str]:
    """提取 ❌ 拒绝 section 下的所有条目 (L69-75)"""
    section = re.search(
        r"###\s*❌\s*拒绝.*?(?=###\s*边界判断|\Z)",
        md, re.DOTALL
    )
    if not section:
        return []
    items = re.findall(r"^-\s+(.+?)$", section.group(0), re.MULTILINE)
    return [item.strip() for item in items]


def extract_boundary_table(md: str) -> list[dict]:
    """提取边界判断表 (markdown table 格式, 跳过 --- 分隔行, 去引号)"""
    section = re.search(
        r"###\s*边界判断.*?(?=##\s*改动前检查表|\Z)",
        md, re.DOTALL
    )
    if not section:
        return []
    rows = re.findall(
        r"^\|\s*(.+?)\s*\|\s*(.+?)\s*\|$",
        section.group(0), re.MULTILINE
    )
    result = []
    for r in rows:
        # 跳过表头 ("想法" | "决策") 和 --- 分隔行
        if r[0] == "想法":
            continue
        if re.match(r"^-+$", r[0].strip()) or re.match(r"^-+$", r[1].strip()):
            continue
        # 去引号: AGENTS.md 里的 idea 用 "..." 包裹, schema.json 里裸字符串
        idea = r[0].strip().strip('"').strip("'")
        decision = r[1].strip()
        result.append({"idea": idea, "decision": decision})
    return result


def extract_checklist(md: str) -> list[str]:
    """提取改动前检查表 (4 个问题)"""
    section = re.search(
        r"##\s*改动前检查表.*?(?=##\s*与其他项目的关系|\Z)",
        md, re.DOTALL
    )
    if not section:
        return []
    items = re.findall(r"^\d+\.\s+(.+?)$", section.group(0), re.MULTILINE)
    return [item.strip() for item in items]


def extract_pitfalls(md: str) -> list[str]:
    """提取已知坑的 bullet 项"""
    section = re.search(
        r"##\s*已知坑.*?(?=##\s*下次开会看什么|\Z)",
        md, re.DOTALL
    )
    if not section:
        return []
    items = re.findall(r"^-\s+(.+?)$", section.group(0), re.MULTILINE)
    return [item.strip() for item in items]


def extract_rule2_trace_template(md: str) -> str | None:
    """从 AGENTS.md 的规则二第 4 步抽取留痕模板文字。

    用于校验 AGENTS.md 与 schema.json 的留痕措辞是否保持一致,
    避免单边修改导致已被废弃的旧措辞复发。
    匹配规则二第 4 步的 "记录留痕" 段,提取引号内的精确文字。
    """
    section = re.search(
        r"###\s*规则二\s*[:：]\s*警告确认流程.*?(?=##\s*硬规则|\Z)",
        md, re.DOTALL,
    )
    if not section:
        return None
    # 规则二 4. 记录留痕 行,提取引号内的模板文字
    m = re.search(
        r"4\.\s*\*\*记录留痕\*\*\s*[:：].*?[“\"](.+?)[”\"]",
        section.group(0), re.DOTALL,
    )
    if not m:
        return None
    # 中文全角引号 / 英文半角引号归一
    return m.group(1).strip()


def extract_all(md: str) -> dict[str, Any]:
    """从 AGENTS.md 抽取所有结构化 section"""
    return {
        "accepted_rules": extract_accepted_rules(md),
        "rejected_rules": extract_rejected_rules(md),
        "boundary_judgments": extract_boundary_table(md),
        "pre_change_checklist": extract_checklist(md),
        "pitfalls": extract_pitfalls(md),
        "rule2_trace_template": extract_rule2_trace_template(md),
    }


def validate_schema(schema: dict[str, Any]) -> list[str]:
    """校验 AGENTS.schema.json 内部一致性"""
    errors = []

    # 必须字段
    required = ["$schema", "schema_version", "project_metadata", "core_rules",
                "project_topology", "agent_protocol", "known_pitfalls",
                "cross_document_refs"]
    for field in required:
        if field not in schema:
            errors.append(f"Missing required field: {field}")

    # core_rules 必须有 accepted + rejected + boundary_judgments
    cr = schema.get("core_rules", {})
    if "accepted_in_core" not in cr:
        errors.append("Missing core_rules.accepted_in_core")
    if "rejected_in_core" not in cr:
        errors.append("Missing core_rules.rejected_in_core")
    if "boundary_judgments" not in cr:
        errors.append("Missing core_rules.boundary_judgments")

    # 边界判断格式: 必须有 idea + decision
    for i, bj in enumerate(cr.get("boundary_judgments", [])):
        if "idea" not in bj or "decision" not in bj:
            errors.append(f"boundary_judgments[{i}] missing idea/decision")

    return errors


def diff_report(extracted: dict[str, Any], schema: dict[str, Any]) -> list[str]:
    """对比 extracted vs schema, 输出需要更新的项"""
    diffs = []

    cr = schema.get("core_rules", {})

    # accepted rules 对比 (用 id 比较)
    schema_accepted_ids = {r["id"] for r in cr.get("accepted_in_core", [])}
    md_accepted_count = len(extracted["accepted_rules"])
    schema_accepted_count = len(cr.get("accepted_in_core", []))
    if md_accepted_count != schema_accepted_count:
        diffs.append(
            f"core_rules.accepted_in_core: AGENTS.md 有 {md_accepted_count} 条, "
            f"schema.json 有 {schema_accepted_count} 条. 数量不一致, 需要重新抽取."
        )

    # rejected rules 对比
    schema_rejected_ids = {r["id"] for r in cr.get("rejected_in_core", [])}
    md_rejected_count = len(extracted["rejected_rules"])
    schema_rejected_count = len(cr.get("rejected_in_core", []))
    if md_rejected_count != schema_rejected_count:
        diffs.append(
            f"core_rules.rejected_in_core: AGENTS.md 有 {md_rejected_count} 条, "
            f"schema.json 有 {schema_rejected_count} 条. 数量不一致, 需要重新抽取."
        )

    # boundary judgments 对比
    schema_bj_ideas = {bj["idea"] for bj in cr.get("boundary_judgments", [])}
    md_bj_ideas = {bj["idea"] for bj in extracted["boundary_judgments"]}
    if schema_bj_ideas != md_bj_ideas:
        only_md = md_bj_ideas - schema_bj_ideas
        only_schema = schema_bj_ideas - md_bj_ideas
        if only_md:
            diffs.append(
                f"boundary_judgments: AGENTS.md 新增但 schema.json 缺失: {sorted(only_md)}"
            )
        if only_schema:
            diffs.append(
                f"boundary_judgments: schema.json 有但 AGENTS.md 缺失: {sorted(only_schema)}"
            )

    # pitfalls 对比 (按数量)
    md_pitfall_count = len(extracted["pitfalls"])
    schema_pitfall_count = len(schema.get("known_pitfalls", {}).get("items", []))
    if md_pitfall_count != schema_pitfall_count:
        diffs.append(
            f"known_pitfalls.items: AGENTS.md 有 {md_pitfall_count} 条, "
            f"schema.json 有 {schema_pitfall_count} 条. 数量不一致, 需要重新抽取."
        )

    # pre_change_checklist 对比
    md_checklist_count = len(extracted["pre_change_checklist"])
    schema_checklist_count = len(schema.get("pre_change_checklist", {}).get("questions", []))
    if md_checklist_count != schema_checklist_count:
        diffs.append(
            f"pre_change_checklist.questions: AGENTS.md 有 {md_checklist_count} 条, "
            f"schema.json 有 {schema_checklist_count} 条. 数量不一致, 需要重新抽取."
        )

    # rule2 trace template 对比 (文字级, 防止单边修改导致 AI 措辞复发)
    md_template = extracted.get("rule2_trace_template")
    schema_template: str | None = None
    try:
        for rule in schema.get("agent_protocol", {}).get("rules", []):
            if rule.get("id") == "warning_confirmation_flow":
                for step in rule.get("steps", []):
                    if step.get("name") == "记录留痕" or step.get("step") == 4:
                        raw_action = step.get("action", "")
                        # 从 schema 的 action 描述中去掉前缀 "在相关变更中说明 "
                        # 以及外层引号,仅保留模板文字本体
                        m = re.search(r"[“\"'](.+?)[”\"']", raw_action)
                        if m:
                            schema_template = m.group(1).strip()
                        break
                break
    except (KeyError, IndexError, TypeError):
        schema_template = None

    if md_template and schema_template and md_template != schema_template:
        diffs.append(
            "agent_protocol.rules[warning_confirmation_flow].steps[记录留痕].action: "
            "AGENTS.md 与 schema.json 的留痕模板文字不一致 (单边修改会导致 AI 措辞复发).\n"
            f"    AGENTS.md : {md_template!r}\n"
            f"    schema.json: {schema_template!r}"
        )
    elif md_template and not schema_template:
        diffs.append(
            "agent_protocol.rules[warning_confirmation_flow].steps[记录留痕].action: "
            "AGENTS.md 能抽出留痕模板, 但 schema.json 找不到对应字段, 请确认结构."
        )
    elif schema_template and not md_template:
        diffs.append(
            "agent_protocol.rules[warning_confirmation_flow].steps[记录留痕]: "
            "schema.json 有留痕模板, 但 AGENTS.md 解析失败, 请检查 md 格式."
        )

    return diffs


def main():
    parser = argparse.ArgumentParser(
        description="从 AGENTS.md 抽取结构化信息, 验证 AGENTS.schema.json 同步状态"
    )
    parser.add_argument(
        "--check", action="store_true",
        help="CI 模式: 有 diff 时退出码 1"
    )
    parser.add_argument(
        "--extract", action="store_true",
        help="只抽取并打印 JSON"
    )
    parser.add_argument(
        "--validate", action="store_true",
        help="只校验 schema.json 合法性"
    )
    args = parser.parse_args()

    if not AGENTS_MD.exists():
        print(f"ERROR: {AGENTS_MD} 不存在", file=sys.stderr)
        sys.exit(1)
    if not SCHEMA_JSON.exists():
        print(f"ERROR: {SCHEMA_JSON} 不存在", file=sys.stderr)
        sys.exit(1)

    md = AGENTS_MD.read_text(encoding="utf-8")
    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))

    # --validate 模式
    if args.validate:
        errors = validate_schema(schema)
        if errors:
            print("❌ AGENTS.schema.json 校验失败:", file=sys.stderr)
            for e in errors:
                print(f"  - {e}", file=sys.stderr)
            sys.exit(1)
        print("✅ AGENTS.schema.json 校验通过")
        sys.exit(0)

    extracted = extract_all(md)

    # --extract 模式
    if args.extract:
        print(json.dumps(extracted, ensure_ascii=False, indent=2))
        sys.exit(0)

    # diff 模式 (默认)
    print("=" * 60)
    print("AGENTS.md → AGENTS.schema.json 同步检查")
    print("=" * 60)
    print(f"AGENTS.md: {AGENTS_MD}")
    print(f"SCHEMA.json: {SCHEMA_JSON}")
    print(f"schema_version: {schema.get('schema_version', 'unknown')}")
    print()

    # 1. 校验 schema 合法性
    schema_errors = validate_schema(schema)
    if schema_errors:
        print("❌ AGENTS.schema.json 内部校验失败:")
        for e in schema_errors:
            print(f"  - {e}")
        print()
    else:
        print("✅ AGENTS.schema.json 内部校验通过")
        print()

    # 2. 抽取 AGENTS.md
    print("从 AGENTS.md 抽取:")
    print(f"  - accepted_rules: {len(extracted['accepted_rules'])} 条")
    print(f"  - rejected_rules: {len(extracted['rejected_rules'])} 条")
    print(f"  - boundary_judgments: {len(extracted['boundary_judgments'])} 条")
    print(f"  - pre_change_checklist: {len(extracted['pre_change_checklist'])} 条")
    print(f"  - pitfalls: {len(extracted['pitfalls'])} 条")
    print()

    # 3. diff
    diffs = diff_report(extracted, schema)
    if diffs:
        print("⚠️  发现不一致 (需要更新 AGENTS.schema.json):")
        for d in diffs:
            print(f"  - {d}")
        print()
        print("更新方法: 改 AGENTS.md 后, 手动同步更新 AGENTS.schema.json,")
        print("或跑本脚本的 --extract 模式获取最新抽取结果, 跟 schema.json 合并.")
    else:
        print("✅ AGENTS.md 跟 AGENTS.schema.json 数量一致")
    print()

    if args.check and (schema_errors or diffs):
        sys.exit(1)
    sys.exit(0)


if __name__ == "__main__":
    main()
