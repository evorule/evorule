# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 EvoRule Project
"""Workflow YAML 重复键检查（fail-closed）。

背景教训：GitHub Actions 对 workflow YAML 的重复顶级键（如两个 `jobs:`）
直接拒绝（schema 校验失败、零 job 执行即 workflow failure），而本地
`yaml.safe_load` 会静默吞掉重复键（后者覆盖前者）无法发现——必须用
重复键感知的加载器显式断言。

用法：
    python check-workflow-yaml.py <repo-root> [...更多 repo-root]
    # 不带参数时检查脚本所在仓 .github/workflows/

对每个 .github/workflows/{*.yml,*.yaml}：
  - 解析失败 / 重复键（任意层级）/ 空文档 -> exit 1 并列出位置
"""
from __future__ import annotations

import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    print("ERROR: PyYAML not installed (pip install pyyaml)")
    sys.exit(2)


class _DupKeyLoader(yaml.SafeLoader):
    """SafeLoader 变体：遇到重复键即抛错，而非静默覆盖。"""


def _no_dup_keys(loader, node, deep=False):
    mapping = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        try:
            duplicate = key in mapping
        except TypeError:
            raise yaml.constructor.ConstructorError(
                None, None,
                f"unhashable mapping key near line {key_node.start_line}",
                key_node.start_mark,
            )
        if duplicate:
            raise yaml.constructor.ConstructorError(
                None, None,
                f"duplicate mapping key {key!r} near line {key_node.start_line}",
                key_node.start_mark,
            )
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


_DupKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _no_dup_keys
)


def check_file(path: Path) -> list[str]:
    problems: list[str] = []
    try:
        docs = list(
            yaml.load_all(path.read_text(encoding="utf-8"), Loader=_DupKeyLoader)
        )
    except yaml.YAMLError as exc:
        return [f"{path}: parse error: {exc}"]
    if not any(d is not None for d in docs):
        problems.append(f"{path}: empty document")
    return problems


def main(argv: list[str]) -> int:
    roots = [Path(a) for a in argv] or [Path(__file__).resolve().parent.parent]
    all_problems: list[str] = []
    checked = 0
    for root in roots:
        wf_dir = root / ".github" / "workflows"
        if not wf_dir.is_dir():
            print(f"[skip] no workflows dir: {wf_dir}")
            continue
        for path in sorted([*wf_dir.glob("*.yml"), *wf_dir.glob("*.yaml")]):
            checked += 1
            all_problems.extend(check_file(path))
    print(f"checked {checked} workflow file(s) under {len(roots)} repo(s)")
    for p in all_problems:
        print(f"  [FAIL] {p}")
    if all_problems:
        print(f"RESULT: FAIL ({len(all_problems)} problem(s))")
        return 1
    print("RESULT: PASS (0 duplicate keys, 0 parse errors)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
