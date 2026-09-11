#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# SPDX-FileCopyrightText: 2026 EvoRule Project Authors
"""L1-1 门禁：server 错误 code 与 console i18n 字典一致性检查。

检查方向（单向）：
  1. server.rs 中所有 `code: Some("XXX")` 错误码必须在前端
     messages.zh.ts 与 messages.en.ts 均存在 `err.XXX` key
     （前端字典允许是 server code 的超集，如预置的 WORKSPACE_*）。
  2. zh/en 两字典的 `err.*` key 集合必须互相一致（防漏译）。
  3. server.rs 必须存在 `fn status_error_code` 映射函数
     （i18n 阶段2 续：裸 StatusCode 错误点经 inject_error_code_middleware
     透传 code）；函数被删或其体内 `Some("...")` 字面量变更即违例。

用法：
  python scripts/check_error_code_i18n.py [--server-rs PATH] [--console-dir PATH]

默认路径按生态工作区兄弟目录约定解析；CI 可用参数覆盖。
退出码：0=通过，1=存在违例（打印违例清单，绝不静默）。
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Set

DEFAULT_SERVER_RS = Path(r"D:\evorule-server\evorule-server\src\api\server.rs")
DEFAULT_CONSOLE_DIR = Path(r"D:\evorule-console-cloud\src\lib\locale")

RE_SERVER_CODE = re.compile(r'code:\s*Some\("([A-Z0-9_]+)"')
RE_SOME_CODE = re.compile(r'Some\("([A-Z0-9_]+)"\)')
RE_ERR_KEY = re.compile(r'"err\.([A-Za-z0-9_]+)"')


def collect_server_codes(server_rs: Path) -> Set[str]:
    text = server_rs.read_text(encoding="utf-8")
    return set(RE_SERVER_CODE.findall(text))


def collect_status_code_block(server_rs: Path) -> tuple[Set[str], bool]:
    """提取 status_error_code 映射函数体内的 Some("...") 字面量（裸 StatusCode 错误点透传）。

    返回 (code 集合, 标记函数是否存在)。函数缺失 = 透传中间件被移除，视为违例。
    """
    text = server_rs.read_text(encoding="utf-8")
    m = re.search(r"fn\s+status_error_code\b", text)
    if not m:
        return set(), False
    end = text.find("\n}", m.start())
    block = text[m.start() : end if end != -1 else len(text)]
    return set(RE_SOME_CODE.findall(block)), True


def collect_err_keys(messages_path: Path) -> Set[str]:
    text = messages_path.read_text(encoding="utf-8")
    return set(RE_ERR_KEY.findall(text))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--server-rs", type=Path, default=DEFAULT_SERVER_RS)
    ap.add_argument("--console-dir", type=Path, default=DEFAULT_CONSOLE_DIR)
    args = ap.parse_args()

    violations: list[str] = []

    server_rs = args.server_rs
    if not server_rs.exists():
        print(f"✗ server.rs 不存在: {server_rs}", file=sys.stderr)
        return 1
    zh_path = args.console_dir / "messages.zh.ts"
    en_path = args.console_dir / "messages.en.ts"
    for p in (zh_path, en_path):
        if not p.exists():
            print(f"✗ 字典文件不存在: {p}", file=sys.stderr)
            return 1

    server_codes = collect_server_codes(server_rs)
    status_codes, marker_present = collect_status_code_block(server_rs)
    if not marker_present:
        violations.append(
            "server.rs 缺少 status_error_code 映射函数"
            "（裸 StatusCode 错误点失去 code 透传，i18n 阶段2 续被移除？）"
        )
    server_codes |= status_codes
    zh_keys = collect_err_keys(zh_path)
    en_keys = collect_err_keys(en_path)

    print(f"server codes ({len(server_codes)}): {sorted(server_codes) or '(none)'}")
    print(f"  其中 ApiResponse 直填: {len(server_codes - status_codes)} / status_error_code 映射: {len(status_codes)}")
    print(f"zh err keys ({len(zh_keys)}) / en err keys ({len(en_keys)})")

    # 检查 1：server code 必须有前端 err.XXX（zh 与 en 各查一遍）
    for code in sorted(server_codes):
        if code not in zh_keys:
            violations.append(f"server code `{code}` 缺少 zh 字典 key `err.{code}`")
        if code not in en_keys:
            violations.append(f"server code `{code}` 缺少 en 字典 key `err.{code}`")

    # 检查 2：zh/en err.* 互相一致（防漏译）
    for k in sorted(zh_keys - en_keys):
        violations.append(f"err key `err.{k}` 仅在 zh 字典，en 缺失")
    for k in sorted(en_keys - zh_keys):
        violations.append(f"err key `err.{k}` 仅在 en 字典，zh 缺失")

    if violations:
        print("\n发现违例：")
        for v in violations:
            print(f"  ✗ {v}")
        return 1

    print("✓ 通过：server code 全部有 zh/en err.* key，且两字典 err.* 对齐")
    return 0


if __name__ == "__main__":
    sys.exit(main())
