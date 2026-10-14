#!/usr/bin/env python3
"""check_constitution_linkage.py：语义路径变更 → PR 描述必须携带「宪法联动」标记。

条文依据：evorule-system-rules 宪法 [CON-R-003] 固化义务（语义→形态方向）——
语义权威（evorule-tcb / evorule-discipline：指令集 / 域 / __exec__ 结构）发生变更时，
必须同批升级 evorule-system-rules 宪法（semver minor+），PR 描述须含「宪法联动」标记行。

判定流程：
    1. 取本 PR diff 文件清单（git diff --name-only <base>...<head>）；
    2. 命中语义路径前缀 → 读取 PR 描述（--body-file 或环境变量）；
    3. 描述中存在已勾选或非复选框形式的「宪法联动：」标记行（冒号后非空）→ 通过；
    4. 未涉语义路径 → 通过；涉语义路径且无标记 → exit 1。

用法：
    python scripts/check_constitution_linkage.py [--base origin/main] [--head HEAD]
        [--body-file PATH] [--env-var PR_BODY] [--skip-if-no-pr]

退出码：
    0 - 未涉语义路径，或标记在场
    1 - 涉语义路径且无标记（红灯阻断）
    2 - 工具自身错（git 失败且未给 --skip-if-no-pr / 参数非法）

CI 挂载：
    - GitHub CI（.github/workflows/ci.yml）：PR 事件注入 PR_BODY，全量历史，强制阻断；
    - Gitee Go（.gitee-ci/validate.yml）：PR 描述环境变量无标准化契约，
      以 --skip-if-no-pr 守护——取得到描述则同等阻断，取不到则 [SKIP]。
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

# 语义路径前缀（指令集 / 域 / __exec__ 结构权威所在 crate）
SEMANTIC_PREFIXES = ("evorule-tcb/", "evorule-discipline/")

# 「宪法联动」标记：允许全角/半角冒号，冒号后须有实质内容
MARKER_RE = re.compile(r"宪法联动\s*[:：]\s*(\S.*)$")

# 复选框前缀（未勾选的模板项不算标记在场）
UNCHECKED_RE = re.compile(r"^\s*[-*]\s*\[\s\]")

COMMENT_RE = re.compile(r"<!--.*?-->", re.DOTALL)


def run_git(args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} 失败: {result.stderr.strip()}")
    return result.stdout


def changed_files(base: str, head: str) -> list[str]:
    """取 base...head 三点 diff（PR 合并基），命中语义路径前缀的文件清单。"""
    out = run_git(["diff", "--name-only", f"{base}...{head}"])
    return [line.strip() for line in out.splitlines() if line.strip()]


def pick_body(args: argparse.Namespace) -> str | None:
    """读 PR 描述：--body-file 优先，其次环境变量。取不到返回 None。"""
    if args.body_file:
        path = os.fsdecode(args.body_file)
        try:
            with open(path, encoding="utf-8") as fh:
                return fh.read()
        except OSError as exc:
            raise RuntimeError(f"读取 PR 描述文件失败 {path}: {exc}") from None
    for name in (args.env_var, "GITEE_PR_BODY"):
        value = os.environ.get(name, "")
        if value.strip():
            return value
    return None


def marker_found(body: str) -> tuple[bool, str]:
    """扫描标记行。返回 (是否在场, 说明)。"""
    cleaned = COMMENT_RE.sub("", body)  # 模板内 HTML 注释指令不算数
    for raw_line in cleaned.splitlines():
        line = raw_line.rstrip()
        if "宪法联动" not in line:
            continue
        if UNCHECKED_RE.match(line):
            return False, "发现未勾选的模板项（- [ ] 宪法联动…）——请勾选或另写标记行"
        if MARKER_RE.search(line):
            return True, line.strip()
    return False, "描述中无「宪法联动：」标记行"


def main() -> int:
    parser = argparse.ArgumentParser(description="语义路径变更 → PR 描述「宪法联动」标记检查")
    parser.add_argument("--base", default=None, help="diff 基准（默认 origin/main，缺则回退 main）")
    parser.add_argument("--head", default="HEAD", help="diff 终点（默认 HEAD）")
    parser.add_argument("--body-file", default=None, help="PR 描述文件路径")
    parser.add_argument("--env-var", default="PR_BODY", help="PR 描述所在环境变量名（默认 PR_BODY）")
    parser.add_argument(
        "--skip-if-no-pr",
        action="store_true",
        help="无 PR 描述或 git diff 不可得时 [SKIP] 退出 0（Gitee Go 无 PR 上下文场景）",
    )
    args = parser.parse_args()

    base = args.base
    if not base:
        for candidate in ("origin/main", "main"):
            try:
                run_git(["rev-parse", "--verify", candidate])
                base = candidate
                break
            except RuntimeError:
                continue
        if not base:
            print("[FAIL] 找不到基准分支（origin/main 与 main 均不可用），无法计算 diff", file=sys.stderr)
            return 2

    try:
        files = changed_files(base, args.head)
    except RuntimeError as exc:
        if args.skip_if_no_pr:
            print(f"[SKIP] 无法计算 diff（{exc}），跳过宪法联动检查")
            return 0
        print(f"[FAIL] {exc}", file=sys.stderr)
        return 2

    semantic = [f for f in files if f.startswith(SEMANTIC_PREFIXES)]
    if not semantic:
        print(f"[OK] 未涉语义路径（{len(files)} 个变更文件），无需宪法联动")
        return 0

    body = pick_body(args)
    if body is None:
        if args.skip_if_no_pr:
            print(
                "[SKIP] 涉语义路径但无 PR 描述上下文（非 PR 流水线），跳过——"
                "强制阻断由 GitHub CI PR 事件承担"
            )
            return 0
        print(
            f"[FAIL] 涉及语义路径 {len(semantic)} 个文件（[CON-R-003] 固化义务），"
            "但取不到 PR 描述（--body-file / 环境变量均空）"
        )
        for f in semantic[:10]:
            print(f"  - {f}")
        return 1

    found, detail = marker_found(body)
    if found:
        print(f"[OK] 语义路径 {len(semantic)} 个文件，宪法联动标记在场：{detail}")
        return 0

    print(f"[FAIL] 涉及语义路径 {len(semantic)} 个文件，{detail}")
    for f in semantic[:10]:
        print(f"  - {f}")
    print(
        "  依宪法 [CON-R-003]：语义变更（evorule-tcb / evorule-discipline 指令集/域/__exec__ 结构）"
        "必须同批升级 evorule-system-rules（semver minor+），"
        "并在 PR 描述写明「宪法联动：<宪法仓 PR/commit 链接>」"
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
