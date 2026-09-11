#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# SPDX-FileCopyrightText: 2026 EvoRule Project Authors
"""L1-2 门禁：docs/ 中英双语完整性与漂移检测。

背景：docs/ 全量双语（2026-09-11），惯例为文末 `---` + `<a id="english"></a>`
+ 英文镜像分节。改中文必同步英文，否则英文静默陈旧。本脚本把"忘了同步"
变成硬报错。

规则（对每个含中文的 docs/**/*.md，不含 build 产物）：
  1. 必须存在 `<a id="english"></a>` 锚点；
  2. 锚点之后英文分节必须有实质内容（至少一个 H1 + 一段正文/表格/列表）；
  3. 漂移粗检：中文分节与英文分节的 H2/H3 数量必须一致。

豁免（EXEMPT，登记于 evorule-i18n 专案 04 文档）：
  - SUMMARY.md：mdBook 导航解析敏感，加英文分节可能破坏 book 构建，
    且 SUMMARY 标题须与各页面主 H1 一致。

用法：
  python scripts/check_docs_bilingual.py [--docs-dir PATH]
退出码：0=通过，1=存在违例。
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

RE_ANCHOR = re.compile(r'^<a id="english"></a>\s*$', re.M)
RE_H2 = re.compile(r"^##\s+\S", re.M)
RE_H3 = re.compile(r"^###\s+\S", re.M)
RE_CJK = re.compile(r"[\u4e00-\u9fff]")
# SPDX 头（文件前几行）不计入正文
RE_SPDX = re.compile(r"^\s*(#!|//|<!--|# SPDX|SPDX-)", re.M)

EXEMPT = {"SUMMARY.md"}  # 豁免文件名（任意层级）

BOOK_DIRS = {"book", "src", "theme"}  # mdBook 构建产物/主题目录，跳过


def is_exempt(path: Path) -> bool:
    return path.name in EXEMPT


def split_bilingual(text: str) -> tuple[str, str | None]:
    """按 english 锚点把文件切成 (中文部分, 英文部分或 None)。"""
    m = RE_ANCHOR.search(text)
    if not m:
        return text, None
    zh = text[: m.start()]
    en = text[m.end():]
    return zh, en


def strip_file_header(text: str) -> str:
    """去掉文件顶部 SPDX/版权注释块（`<!--` ... `-->` 或 # 开头注释行）。"""
    lines = text.splitlines(keepends=True)
    i = 0
    if lines and lines[0].lstrip().startswith("<!--"):
        while i < len(lines) and "-->" not in lines[i]:
            i += 1
        i += 1
        return "".join(lines[i:])
    return text


def heading_counts(text: str) -> tuple[int, int]:
    return len(RE_H2.findall(text)), len(RE_H3.findall(text))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--docs-dir", type=Path, default=Path(__file__).resolve().parent.parent / "docs")
    args = ap.parse_args()

    docs = args.docs_dir
    if not docs.is_dir():
        print(f"✗ docs 目录不存在: {docs}", file=sys.stderr)
        return 1

    violations: list[str] = []
    checked = 0

    for path in sorted(docs.rglob("*.md")):
        if is_exempt(path):
            continue
        if any(part in BOOK_DIRS for part in path.relative_to(docs).parts[:-1]):
            continue
        text = path.read_text(encoding="utf-8")
        body = strip_file_header(text)
        if not RE_CJK.search(body):
            continue  # 纯英文文件（未来若有）不要求双语
        checked += 1
        zh, en = split_bilingual(body)

        if en is None:
            violations.append(f"{path.relative_to(docs)}: 含中文但缺少 <a id=\"english\"></a> 英文分节")
            continue

        en_trim = en.strip()
        # 英文分节实质内容：去掉标题行后仍有内容
        en_body = "\n".join(
            ln for ln in en_trim.splitlines() if not ln.lstrip().startswith("#")
        ).strip()
        if not en_body:
            violations.append(f"{path.relative_to(docs)}: 英文分节为空（仅标题）")
            continue

        zh_h2, zh_h3 = heading_counts(zh)
        en_h2, en_h3 = heading_counts(en_trim)
        if (zh_h2, zh_h3) != (en_h2, en_h3):
            violations.append(
                f"{path.relative_to(docs)}: 章节结构漂移 "
                f"zh H2={zh_h2}/H3={zh_h3} vs en H2={en_h2}/H3={en_h3}"
                "（改中文后未同步英文？）"
            )

    print(f"已检查含中文 docs 文件: {checked} 篇（豁免: {sorted(EXEMPT)}）")
    if violations:
        print(f"\n发现违例 {len(violations)} 条：")
        for v in violations:
            print(f"  ✗ {v}")
        return 1

    print("✓ 通过：全部含中文 docs 文件均有英文分节且章节结构对齐")
    return 0


if __name__ == "__main__":
    sys.exit(main())
