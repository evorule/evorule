#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# =============================================================================
# check_assurance_compliance.py — EvoRule ASSURANCE.md 规范合规检查器
#
# 目的：把 verification/ASSURANCE.md 当作规范文件即时执法——任何文档生成、
# 更新、代码变更在提交期/CI 即时校验，不做事后专项检查（规范/状态分离：
# ASSURANCE.md §0.4；条款准入与禁止写入清单：§0.5；对外表述纪律：§0.7）。
#
# 规则（T 系）：
#   T1 规范自体状态词零容忍（§0.5）：ASSURANCE.md 第 0–9 章正文不得出现
#      五档状态词/达成表述/百分比；### 0.5 元规则区与附录 A 之后资料性
#      内容豁免；规范性自述行（不承载/不得/不复述等）豁免
#   T2 规范自体 SHA 零容忍（§0.5）：ASSURANCE.md 全文不得出现裸 hex SHA
#   T3 内部信息零容忍（M9）：扫描范围内不得出现本机盘符路径、内部
#      工作区路径、knowledge 路径、邮箱 PII（evorulelab@gmail.com 为项目
#      主动公开的联系方式，豁免）
#   T4 比较级禁令（§0.7/NC-15）：对外材料行内同时出现认证体系名与比较
#      断言词即违规；否定自指行（不主张/不得/不可换算等）豁免
#   T5 等级冒领禁令（§0.4 禁令3）：「已达到/已实现/已达成 ALn」仅允许
#      出现在 STATUS.md（状态唯一真相源）与 ASSURANCE.md（规范自体），
#      其余文件出现即违规
#   T6 状态承载纪律（M2/M1）：verification/ 顶层文档不得独立断言验证
#      状态——出现五档 emoji 或状态词即违规（MECHANISM.md M2 词汇定义
#      节区与规范性规则行豁免；plan/ 方案文档子树豁免——历史层覆盖标记
#      为文档自身披露的冻结口径「不构成当前状态断言」，数字对齐由 S9 执法）
#
# 扫描范围：根目录 *.md + docs/**/*.md + verification/**/*.md
#          （evidence/ 证据库由 M3/S 系管辖，不在本检查器范围）
# 规则↔文件：
#   ASSURANCE.md                    → T1 T2 T3
#   STATUS.md / DISCLOSURE_LOG.md   → T3（状态事务承载，T4/T5/T6 豁免）
#   其余扫描范围文件                → T3 T4 T5；verification/ 下另加 T6
#
# 用法：
#   python scripts/check_assurance_compliance.py                  # 全仓扫描
#   python scripts/check_assurance_compliance.py --files a.md b   # 增量（hook 用）
#   python scripts/check_assurance_compliance.py --warn           # 仅输出，exit 0
#   python scripts/check_assurance_compliance.py --json           # JSON 输出
#
# 退出码：
#   0 = 全部通过；1 = 存在违规（--warn 时恒为 0）；2 = 环境错误（文件不可读等）
# =============================================================================

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

ASSURANCE_REL = "verification/ASSURANCE.md"
STATUS_REL = "verification/STATUS.md"
DISCLOSURE_REL = "verification/DISCLOSURE_LOG.md"
EVIDENCE_PREFIX = "verification/evidence/"

# ---------------------------------------------------------------------------
# T1：ASSURANCE.md 规范正文状态词（区域扫描）
# ---------------------------------------------------------------------------

# 区域锚点：### 0.5 元规则区（允许引用禁词作规则说明）→ ## 第 1 章；附录 A 起资料性
H_META_START = re.compile(r"^###\s+0\.5\b")
H_CH1 = re.compile(r"^##\s+第\s*1\s*章")
H_APPENDIX_A = re.compile(r"^##\s+附录\s*A\b")

# T1 行级豁免：规范性自述行（描述规则本身，非状态断言）
T1_META_LINE_HINTS = re.compile(
    r"(不承载|不得|不含|不复述|不主张|不承诺|不作|唯一权威|唯一真相|禁止|禁令|词汇|定义)"
)

T1_PATTERNS = [
    ("五档状态emoji", re.compile(r"[✅🟡🔵⏳❌]")),
    ("当前达成/缺口叙述", re.compile(r"当前(?:达成|缺口|止于|止步)")),
    ("已达成表述", re.compile(r"(?:已|业已)达成")),
    ("达成率", re.compile(r"达成率")),
    ("百分比", re.compile(r"\d+\s*%")),
]

# ---------------------------------------------------------------------------
# T2：ASSURANCE.md 全文裸 hex SHA（要求含至少一位数字，避免纯 hex 字母英文词误报；
#     \b 保证不命中更长 hex/字母数字 token 的内部片段）
# ---------------------------------------------------------------------------
T2_SHA_RE = re.compile(r"\b(?=[0-9a-f]*[0-9])[0-9a-f]{7,40}\b")

# ---------------------------------------------------------------------------
# T3：内部信息零容忍
# ---------------------------------------------------------------------------
T3_PATTERNS = [
    ("本机盘符路径", re.compile(r"(?<![A-Za-z])[A-Za-z]:[\\/]")),  # 负向后视排除 https:// 等 URL scheme
    # 内部工作区路径（拼接构造，源码不携带工作区目录名字面量）
    ("内部工作区路径", re.compile(r"\.tra" + "e")),
    ("knowledge 内部路径", re.compile(r"knowledge[\\/]")),
]
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
# 项目主动公开的联系方式（CLA/SECURITY/CONTRIBUTING/CHANGELOG 等对外文件署名），豁免
PUBLIC_CONTACT = "evorulelab@gmail.com"

# ---------------------------------------------------------------------------
# T4：对外材料比较级禁令（§0.7/NC-15）
# ---------------------------------------------------------------------------
T4_CERT_RE = re.compile(
    r"(DO-178C|DO-333|DO-248|Common Criteria|EAL\s?\d|ISO\s*26262|IEC\s*61508|ASIL\s?[A-D]?|SIL\s?\d)"
)
T4_COMP_RE = re.compile(r"(超过|超越|强于|高于|优于|等同|等价于|不低于|不逊于|相当|持平|同级)")
# 否定自指行豁免：声明「不作比较主张」的行不是违规
T4_NEG_RE = re.compile(r"(不主张|不得|不构成|不可换算|未申请|不承诺|不作|不视为|并非|不是|禁用|禁止)")

# ---------------------------------------------------------------------------
# T5：等级冒领禁令（§0.4 禁令3）
# ---------------------------------------------------------------------------
T5_CLAIM_RE = re.compile(r"(?:已达到|已实现|已达成)\s*AL\s*[0-4]")

# ---------------------------------------------------------------------------
# T6：verification/ 文档状态承载纪律（M2）
# ---------------------------------------------------------------------------
T6_EMOJI_RE = re.compile(r"[✅🟡🔵⏳❌]")
T6_WORD_RE = re.compile(r"(当前实跑|历史\s?PASS|间接覆盖|不可运行)")
T6_LINE_HINTS = re.compile(r"(定义|词汇|五档|M2|只能引用)")

H_ANY = re.compile(r"^#{1,4}\s")
H_M2 = re.compile(r"^#{1,4}\s.*\bM2\b")


class EnvError(Exception):
    """环境错误（exit 2）。"""


class Report:
    def __init__(self):
        self.items = []

    def error(self, rule, message):
        self.items.append({"rule": rule, "severity": "error", "message": message})

    @property
    def has_errors(self):
        return bool(self.items)


def collect_files():
    """默认全仓扫描集：根 *.md + docs/**/*.md + verification/**/*.md（除 evidence/）。"""
    files = set()
    for p in REPO_ROOT.glob("*.md"):
        files.add(p.relative_to(REPO_ROOT).as_posix())
    for base in ("docs", "verification"):
        root = REPO_ROOT / base
        if not root.is_dir():
            continue
        for p in root.rglob("*.md"):
            rel = p.relative_to(REPO_ROOT).as_posix()
            if rel.startswith(EVIDENCE_PREFIX):
                continue
            files.add(rel)
    files.add(ASSURANCE_REL)
    return sorted(files)


def read_lines(rel):
    path = REPO_ROOT / rel
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        raise EnvError(f"文件不可读: {rel} ({e})")
    return text.splitlines()


def t1_region(lines):
    """T1 扫描区域：第 0–9 章正文（跳过 ### 0.5 元规则区与附录 A 之后资料性内容）。"""
    out = []
    in_meta = False
    in_appendix = False
    for line_no, line in enumerate(lines, 1):
        if H_META_START.match(line):
            in_meta = True
            continue
        if H_CH1.match(line):
            in_meta = False
            continue
        if H_APPENDIX_A.match(line):
            in_appendix = True
        if in_meta or in_appendix:
            continue
        out.append((line_no, line))
    return out


def t6_region(rel, lines):
    """T6 扫描区域：MECHANISM.md 的 M2 词汇定义节区豁免（定义行属合法定义）。"""
    if rel != "verification/MECHANISM.md":
        return list(enumerate(lines, 1))
    out = []
    in_m2 = False
    for line_no, line in enumerate(lines, 1):
        if H_ANY.match(line):
            in_m2 = bool(H_M2.match(line))
            continue
        if in_m2:
            continue
        out.append((line_no, line))
    return out


def clip(line, width=60):
    line = line.strip()
    return line if len(line) <= width else line[: width - 1] + "…"


def scan_file(rel, report):
    if not rel.endswith(".md"):  # 文档检查器：--files 增量模式传入非文档文件时跳过
        return
    lines = read_lines(rel)
    if lines is None:  # --files 增量模式给了扫描集外的文件（如 .rs）→ 跳过
        return
    is_assurance = rel == ASSURANCE_REL
    is_status = rel == STATUS_REL
    is_disclosure = rel == DISCLOSURE_REL
    in_verification = rel.startswith("verification/")

    # ---- T1：规范自体状态词（区域 + 行级豁免）----
    if is_assurance:
        for line_no, line in t1_region(lines):
            if T1_META_LINE_HINTS.search(line):
                continue
            for name, pat in T1_PATTERNS:
                if pat.search(line):
                    report.error("T1", f"{rel}:{line_no}: 规范正文出现状态/达成表述（{name}）: 「{clip(line)}」")

        # ---- T2：规范自体裸 SHA（全文零容忍）----
        for line_no, line in enumerate(lines, 1):
            m = T2_SHA_RE.search(line)
            if m:
                report.error("T2", f"{rel}:{line_no}: 规范正文出现裸 hex SHA「{m.group(0)}」: 「{clip(line)}」")

    # ---- T3：内部信息零容忍（全扫描集）----
    for line_no, line in enumerate(lines, 1):
        probe = line.replace(PUBLIC_CONTACT, "")  # 项目公开联系方式豁免
        for name, pat in T3_PATTERNS:
            m = pat.search(probe)
            if m:
                report.error("T3", f"{rel}:{line_no}: 出现{name}「{m.group(0)}」: 「{clip(line)}」")
        m = EMAIL_RE.search(probe)
        if m:
            report.error("T3", f"{rel}:{line_no}: 出现邮箱 PII「{m.group(0)}」: 「{clip(line)}」")

    # ---- T4：对外材料比较级禁令 ----
    if not (is_assurance or is_status or is_disclosure):
        for line_no, line in enumerate(lines, 1):
            if T4_CERT_RE.search(line) and T4_COMP_RE.search(line) and not T4_NEG_RE.search(line):
                report.error("T4", f"{rel}:{line_no}: 对认证体系作比较断言（§0.7/NC-15）: 「{clip(line)}」")

    # ---- T5：等级冒领禁令 ----
    if not (is_assurance or is_status):
        for line_no, line in enumerate(lines, 1):
            m = T5_CLAIM_RE.search(line)
            if m:
                report.error("T5", f"{rel}:{line_no}: 等级冒领「{m.group(0)}」（§0.4 禁令3，状态见 STATUS.md）: 「{clip(line)}」")

    # ---- T6：verification/ 顶层文档状态承载纪律 ----
    # plan/ 子树豁免：方案/白皮书区的历史层覆盖标记为文档自身披露的冻结口径
    # （plan v3 §修订说明：不构成当前状态断言），其数字对齐由 S 系 S9 执法。
    is_verification_top = rel.startswith("verification/") and rel.count("/") == 1
    if is_verification_top and not (is_assurance or is_status or is_disclosure):
        for line_no, line in t6_region(rel, lines):
            if T6_LINE_HINTS.search(line):
                continue
            if T6_EMOJI_RE.search(line):
                report.error("T6", f"{rel}:{line_no}: 出现五档状态 emoji（M2 仅 STATUS.md 承载）: 「{clip(line)}」")
            elif T6_WORD_RE.search(line):
                report.error("T6", f"{rel}:{line_no}: 出现五档状态词（M2 仅 STATUS.md 承载）: 「{clip(line)}」")


def main():
    ap = argparse.ArgumentParser(description="ASSURANCE.md 规范合规检查器（T 系规则）")
    ap.add_argument("--files", nargs="*", metavar="FILE",
                    help="只检查指定文件（增量模式，供 pre-commit hook 使用；默认全仓扫描）")
    ap.add_argument("--warn", action="store_true", help="仅输出违规，exit 总是 0")
    ap.add_argument("--json", action="store_true", help="JSON 输出（CI 消费）")
    args = ap.parse_args()

    try:
        if args.files:
            targets = []
            for f in args.files:
                rel = Path(f).relative_to(REPO_ROOT).as_posix() if Path(f).is_absolute() \
                    else Path(f).as_posix()
                targets.append(rel)
        else:
            targets = collect_files()
    except ValueError as e:
        print(f"[ASSURANCE] 环境错误: {e}", file=sys.stderr)
        return 2

    report = Report()
    try:
        for rel in targets:
            scan_file(rel, report)
    except EnvError as e:
        print(f"[ASSURANCE] 环境错误: {e}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps({
            "ok": not report.has_errors,
            "checked_files": len(targets),
            "violations": report.items,
        }, ensure_ascii=False, indent=2))
    else:
        if not report.has_errors:
            print(f"[ASSURANCE] 已检查 {len(targets)} 个文件，T1–T6 全部通过。")
        else:
            print(f"[ASSURANCE] 违规 {len(report.items)} 项：")
            for item in report.items:
                print(f"  [{item['rule']}] {item['message']}")
            print("[ASSURANCE] 规范依据：verification/ASSURANCE.md（§0.4/§0.5/§0.7、NC-15、M2）；"
                  "处置指引：小项直接修，规范性偏离登记 STATUS.md §3.2 DEV-x。")

    if report.has_errors and not args.warn:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
