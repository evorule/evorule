#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# =============================================================================
# check_status_sync.py — EvoRule 验证状态同步一致性检查器
#
# 目的：防止「三层漂移」（代码/CI ↔ 证据库 ↔ 验证文档）复发。
# 以 verification/STATUS.md 为唯一真相源（MECHANISM.md M1），做四个方向的双向核对：
#   状态 → 证据：✅ 行的证据必须存在且可匹配（M2 判定条件②的执法）
#   证据 → 状态：证据库最新结果落盘后，状态行必须同步（M1 常态化）
#   文档 → CI 实况：kani.yml / Cargo.toml 与 STATUS.md 附录清单一致（M1.3）
#   变更 → 披露：STATUS.md 变更须有 DISCLOSURE_LOG.md 留痕（M5）
#
# 规则（S 系，条款依据见 MECHANISM.md M 系）：
#   S1  状态词汇合规（M2）：主状态列仅允许五档（✅🟡🔵⏳❌）及复合
#   S2  状态→证据齐备（M2/M3.1）：✅ 行证据列非空；声明的证据文件/模式
#       在证据库（非隔离区）中可匹配；「N 份」计数声明核对
#   S3  证据→状态闭环（M1）：证据库中每个 proof 的最新证据（按时间戳）
#       与对应属性行主状态一致；孤儿证据（无属性行/行内未提及）须登记
#   S4  证据命名与配对（M3.1/M3.3）：文件名符合 <属性号>[.<harness>]_
#       <PASS|FAIL>_<sha>_<ts>.log 规范；.log 与 .stdout.txt 成对；非 0KB
#   S5  FAIL 证据强制字段（M3.2）：内容须含 harness/测试名 + 完整复现命令
#   S6  隔离区 README（M3.5）：每个 _invalidated/ 须有 README.md
#   S7  证据时效（M3.4）：PASS 证据基线不早于对应 proof 函数最后一次
#       变更（git log -L 函数级；未命中时降级文件级并注明）
#   S8  CI 闸门与清单对齐（M1.3/M8）：kani.yml 各 job 的 harness 集合 =
#       STATUS.md 附录 B/C 清单 = Cargo.toml [package.metadata.kani]；
#       proof 源码函数全集须全部登记（新增 proof 未登记即报）
#   S9  对外数字对齐（M1/M6）：README badge/正文、ROADMAP、plan v3、
#       verification/README、reactor KANI.md 的数字与附录推导值一致；
#       证据库 PASS proof 数交叉验证
#   S10 版本对齐（M4）：STATUS.md 快照版本 = Cargo.toml workspace version
#       = MECHANISM.md 版本对齐声明
#   S11 披露留痕联动（M5）：DISCLOSURE_LOG.md 最后修改不早于 STATUS.md；
#       工作区未提交变更中两者须成对出现
#
# 说明：M2 将 ✅ 定义为「当前版本可运行 + 归档证据」双条件齐备，故不设
# 「待归档宽限」——无证据的 ✅ 即违规（S2），不允许临时性空档。
#
# 用法：
#   python scripts/check_status_sync.py                 # 默认 = --strict
#   python scripts/check_status_sync.py --warn          # 仅输出，exit 总是 0
#   python scripts/check_status_sync.py --skip-git      # 跳过 S7/S11（非 git 环境）
#   python scripts/check_status_sync.py --json          # JSON 输出（CI 消费）
#   python scripts/check_status_sync.py --repo <path>   # 指定仓库根（仓外运行）
#
# 退出码：
#   0 = 全部通过；1 = 存在违规（--strict 模式）；2 = 环境错误
#      （STATUS.md 缺失、证据目录不可读、git 不可用等）
# =============================================================================

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

TCB_PROOF_SRC = "evorule-tcb/tests/kani/kani_proofs.rs"
REACTOR_PROOF_SRC = "evorule-reactor/verification/kani_proofs.rs"

# ---------------------------------------------------------------------------
# 模式定义
# ---------------------------------------------------------------------------

# M2 五档（允许复合，如 ❌+🔵）
STATUS_CELL_RE = re.compile(r'^(?:[✅🟡🔵⏳❌]\s*\+\s*)*[✅🟡🔵⏳❌]$')

# M3.1 证据文件名：<属性号>[.<harness>]_<STATUS>_<sha>_<ts>.log / .stdout.txt
# 属性号可为复合（如 P0-9-P0-10 覆盖两个属性）
EV_NAME_RE = re.compile(
    r'^(P[01]-\d+(?:-P[01]-\d+)?)'
    r'(?:\.([A-Za-z0-9_]+))?'
    r'_(PASS|FAIL)_([0-9a-f]{7,40})_(\d{8}_\d{6})'
    r'\.(log|stdout\.txt)$'
)

# STATUS.md 证据列中的证据 token（含通配写法：<harness> / 尾部 *）
EV_TOKEN_RE = re.compile(
    r'(P[01]-\d+(?:-P[01]-\d+)?)'
    r'(?:\.([\w<>]+))?'
    r'_(PASS|FAIL)_([0-9a-f]{7,40})_(\d{8}_\d{6})'
    r'(\*)?'
)

# proof 源码中的 Kani 证明函数（kani::proof 之后可能带 #[kani::unwind(N)] 等二级属性）
KANI_PROOF_FN_RE = re.compile(
    r'#\[[^\]]*kani::proof[^\]]*\](?:\s*#\[[^\]]*\])*\s*(?:pub\s+)?fn\s+(\w+)')

ATTR_RE = re.compile(r'P[01]-\d+')


class EnvError(Exception):
    """环境错误（exit 2）。"""


class Report:
    def __init__(self):
        self.items = []  # dict: rule / severity / message

    def error(self, rule, message):
        self.items.append({"rule": rule, "severity": "error", "message": message})

    def note(self, rule, message):
        self.items.append({"rule": rule, "severity": "note", "message": message})

    def errors(self):
        return [i for i in self.items if i["severity"] == "error"]


# ---------------------------------------------------------------------------
# git 辅助
# ---------------------------------------------------------------------------

class Git:
    def __init__(self, enabled):
        self.enabled = enabled
        self._func_cache = {}

    def run(self, args):
        if not self.enabled:
            return 1, ""
        try:
            p = subprocess.run(
                ["git"] + args, capture_output=True, text=True,
                encoding="utf-8", errors="replace", cwd=str(REPO_ROOT),
            )
            return p.returncode, p.stdout
        except OSError:
            raise EnvError("git 不可用（可用 --skip-git 跳过 S7/S11）")

    def last_change(self, path):
        """文件最后一次变更（commit, committer_ts），无历史返回 None。"""
        rc, out = self.run(["log", "-1", "--format=%H %ct", "--", path])
        if rc != 0 or not out.strip():
            return None
        parts = out.split()
        return parts[0], int(parts[1])

    def porcelain(self, path):
        rc, out = self.run(["status", "--porcelain", "--", path])
        return bool(rc == 0 and out.strip())

    def func_last_change(self, proof_file, harness):
        r"""proof 函数最后一次变更 commit，失败返回 None。

        优先用正则范围（/fn NAME[^[:alnum:]_]/ 到 /^}/）精确界定函数体：
        - :funcname: 形式的范围终止于下一个函数起始行，会把下一函数的
          doc 注释圈进来（实测：03643aa 为 P0-11 新增的注释被归入
          max_rounds_termination，造成误报）；
        - POSIX ERE 不接受 \( 转义括号，用字符类 [^[:alnum:]_] 替代。
        """
        key = (proof_file, harness)
        if key in self._func_cache:
            return self._func_cache[key]
        val = None
        spec = "/fn " + harness + "[^[:alnum:]_]/,/^}/:" + proof_file
        rc, out = self.run(["log", "--format=%H", "-1", "-L", spec])
        if rc == 0 and out.strip():
            val = out.splitlines()[0].strip()
        else:  # 降级：funcname 形式（可能把相邻注释算进来，判定从严）
            rc, out = self.run(["log", "--format=%H", "-1", "-L", f":{harness}:{proof_file}"])
            if rc == 0 and out.strip():
                val = out.splitlines()[0].strip()
        self._func_cache[key] = val
        return val

    def file_last_change(self, proof_file):
        rc, out = self.run(["log", "--format=%H", "-1", "--", proof_file])
        return out.splitlines()[0].strip() if (rc == 0 and out.strip()) else None

    def commit_exists(self, sha):
        rc, _ = self.run(["rev-parse", "--verify", "--quiet", sha + "^{commit}"])
        return rc == 0

    def is_ancestor(self, a, b):
        rc, _ = self.run(["merge-base", "--is-ancestor", a, b])
        return rc == 0


# ---------------------------------------------------------------------------
# 解析：STATUS.md
# ---------------------------------------------------------------------------

def parse_status(text):
    """返回 (rows, sections)：rows = 属性号 -> 行 dict；sections = 分区行列表。"""
    rows = {}
    sections = {"P0": [], "P1": [], "appB": [], "appC": []}
    current = None
    line_no = 0
    for line in text.splitlines():
        line_no += 1
        if line.startswith("## "):
            if "一、P0" in line:
                current = "P0"
            elif "二、P1" in line:
                current = "P1"
            elif "附录 B" in line:
                current = "appB"
            elif "附录 C" in line:
                current = "appC"
            else:
                current = None
            continue
        if current:
            sections[current].append((line_no, line))
            if current in ("P0", "P1") and line.startswith("|"):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) >= 9 and ATTR_RE.fullmatch(cells[0]):
                    attr = cells[0]
                    rows[attr] = {
                        "attr": attr,
                        "name": cells[1],
                        "status": cells[3],
                        "proofs": cells[5],
                        "evidence": cells[6],
                        "line_no": line_no,
                        "text": line,
                    }
    return rows, sections


def parse_appendix_b(section_lines):
    """附录 B → (a_declared, a_names, b_declared, b_names)。"""
    text = "\n".join(l for _, l in section_lines)
    ma = re.search(r"\*\*A 档 (\d+) 个\*\*", text)
    mb = re.search(r"\*\*B 档 (\d+) 个\*\*", text)
    if not ma or not mb:
        raise EnvError("STATUS.md 附录 B 缺少「A 档 N 个」/「B 档 N 个」标题")
    a_text = text[ma.end():mb.start()]
    b_text = text[mb.start():]
    return (
        int(ma.group(1)), set(re.findall(r"`(\w+)`", a_text)),
        int(mb.group(1)), set(re.findall(r"`(\w+)`", b_text)),
    )


def parse_appendix_c(section_lines):
    """附录 C → (ci_declared, ci_names, nonci_declared, nonci_names)。"""
    ci_names, nonci_names = set(), set()
    ci_declared = nonci_declared = None
    for _, line in section_lines:
        m_ci = re.search(r"入 kani\.yml PR 闸门（(\d+) 个）", line)
        m_nc = re.search(r"未入 CI（(\d+) 个）", line)
        names = set(re.findall(r"`(\w+)`", line))
        if m_ci:
            ci_declared = int(m_ci.group(1))
            ci_names |= names
        elif m_nc:
            nonci_declared = int(m_nc.group(1))
            nonci_names |= names
    if ci_declared is None or nonci_declared is None:
        raise EnvError("STATUS.md 附录 C 缺少「入 kani.yml PR 闸门（N 个）」/「未入 CI（N 个）」行")
    return ci_declared, ci_names, nonci_declared, nonci_names


# ---------------------------------------------------------------------------
# 解析：证据库
# ---------------------------------------------------------------------------

def iter_evidence_files():
    """非隔离区证据文件 → (Path, rel_posix)。"""
    roots = sorted(REPO_ROOT.glob("evorule-*/verification/evidence"))
    if not roots:
        raise EnvError("未找到任何 evorule-*/verification/evidence/ 证据目录")
    for root in roots:
        for f in sorted(root.rglob("*")):
            if not f.is_file():
                continue
            rel = f.relative_to(REPO_ROOT).as_posix()
            if "/_invalidated/" in rel:
                continue
            yield f, rel


def build_evidence_index():
    """返回 (records, log_basenames)。record 见 EV_NAME_RE 各捕获组。"""
    records = []
    for f, rel in iter_evidence_files():
        m = EV_NAME_RE.match(f.name)
        if not m:
            continue  # 不合规文件名由 S4 统一报告，此处跳过
        records.append({
            "path": f,
            "rel": rel,
            "attr": m.group(1),
            "harness": m.group(2),
            "status": m.group(3),
            "sha": m.group(4),
            "ts": m.group(5),
            "size": f.stat().st_size,
            "kind": "log" if f.name.endswith(".log") else "stdout",
        })
    log_basenames = {r["path"].name for r in records if r["kind"] == "log"}
    return records, log_basenames


def expand_attrs(attr):
    """P0-9-P0-10 → [P0-9, P0-10]。"""
    return ATTR_RE.findall(attr)


# ---------------------------------------------------------------------------
# 解析：proof 源码 / kani.yml / Cargo.toml
# ---------------------------------------------------------------------------

def parse_proof_source(rel_path):
    path = REPO_ROOT / rel_path
    if not path.exists():
        raise EnvError(f"proof 源码缺失：{rel_path}")
    text = path.read_text(encoding="utf-8", errors="replace")
    return set(KANI_PROOF_FN_RE.findall(text))


def parse_kani_yml_jobs(yml_path):
    if not yml_path.exists():
        raise EnvError(f"缺失：{yml_path}")
    jobs = {}
    current = None
    in_jobs = False
    for line in yml_path.read_text(encoding="utf-8", errors="replace").splitlines():
        if re.match(r"^jobs:\s*$", line):
            in_jobs = True
            continue
        if not in_jobs:
            continue
        if line.lstrip().startswith("#"):
            continue  # 注释不参与 harness 归属（避免 job 头注释里的 proof 名误报）
        m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if m:
            current = m.group(1)
            jobs[current] = []
            continue
        if current is not None and line.startswith(" "):
            jobs[current].append(line)
        elif current is not None and line and not line.startswith(" "):
            in_jobs = False
            current = None
    return {k: "\n".join(v) for k, v in jobs.items()}


def parse_cargo_kani_proofs():
    """各 crate Cargo.toml 中 [package.metadata.kani] 的 proofs 列表。"""
    result = {}
    for toml in sorted(REPO_ROOT.glob("evorule-*/Cargo.toml")):
        text = toml.read_text(encoding="utf-8", errors="replace")
        m = re.search(r"\[package\.metadata\.kani\](.*?)(?=^\[|\Z)", text, re.S | re.M)
        if not m:
            continue
        pm = re.search(r"proofs\s*=\s*\[(.*?)\]", m.group(1), re.S)
        if pm:
            result[toml.parent.name] = set(re.findall(r'"(\w+)"', pm.group(1)))
    return result


def cargo_workspace_version(cargo_toml_path):
    text = cargo_toml_path.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    body = m.group(1) if m else text
    vm = re.search(r'^version\s*=\s*"(\d+\.\d+\.\d+)"', body, re.M)
    return vm.group(1) if vm else None


# ---------------------------------------------------------------------------
# S 系规则
# ---------------------------------------------------------------------------

def rule_s1(rows, rep):
    """S1 状态词汇合规（M2）。"""
    for attr, row in sorted(rows.items()):
        status = row["status"]
        if not STATUS_CELL_RE.match(status):
            if "🔧" in status:
                rep.error("S1", f"STATUS.md:{row['line_no']} {attr}：主状态含旧三档词汇「🔧」（M2 禁止）")
            else:
                rep.error("S1", f"STATUS.md:{row['line_no']} {attr}：主状态「{status}」不符合五档词汇（M2）")


def rule_s2(rows, log_basenames, rep):
    """S2 状态→证据齐备（M2/M3.1）：✅ 行证据列非空且可匹配。"""
    for attr, row in sorted(rows.items()):
        cell = row["evidence"]
        if "✅" in row["status"] and cell in ("", "—"):
            rep.error("S2", f"STATUS.md:{row['line_no']} {attr}：主状态为 ✅ 但证据列为空"
                            f"（M2 双条件：可运行 + 归档证据，不允许无证据空档）")
            continue
        counts = re.findall(r"(\d+)\s*份", cell)
        for m in EV_TOKEN_RE.finditer(cell):
            attr_id, harness, status, sha, ts, star = m.groups()
            tok = m.group(0)
            is_wild = (harness and ("<" in harness or "*" in harness)) or star
            if is_wild:
                hpat = "*" if (not harness or "<" in harness) else harness
                pattern = "{}.{}_{}_{}_{}{}".format(attr_id, hpat, status, sha, ts, "*" if star else "")
                matches = [n for n in log_basenames if fnmatch.fnmatch(n, pattern)]
                if not matches:
                    rep.error("S2", f"STATUS.md:{row['line_no']} {attr}：证据模式「{tok}」在证据库（非隔离区）中无匹配文件")
                if counts:
                    declared = int(counts[0])
                    if declared != len(matches):
                        rep.error("S2", f"STATUS.md:{row['line_no']} {attr}：声明 {declared} 份证据，"
                                        f"实际匹配 {len(matches)} 份（模式「{tok}」）")
            else:
                if harness:
                    base = "{}.{}_{}_{}_{}.log".format(attr_id, harness, status, sha, ts)
                else:
                    base = "{}_{}_{}_{}.log".format(attr_id, status, sha, ts)
                if base not in log_basenames:
                    rep.error("S2", f"STATUS.md:{row['line_no']} {attr}：证据文件「{base}」不存在于证据库（非隔离区）")


def rule_s3(rows, ev_index, rep):
    """S3 证据→状态闭环（M1）：最新证据与行状态一致；孤儿证据须登记。"""
    kani_groups = {}   # (attr, harness) -> latest record
    diff_groups = {}   # attr -> latest record
    for rec in ev_index:
        if rec["kind"] != "log":
            continue
        if rec["harness"]:
            key = (rec["attr"], rec["harness"])
            g = kani_groups
        else:
            key = rec["attr"]
            g = diff_groups
        if key not in g or rec["ts"] >= g[key]["ts"]:
            g[key] = rec

    # Kani 证据：最新 PASS → 行主状态应含 ✅；最新 FAIL → 不得为 ✅
    for (attr, harness), rec in sorted(kani_groups.items()):
        if attr not in rows:
            rep.error("S3", f"证据 {rec['path'].name}：属性 {attr} 在 STATUS.md 中无对应行（孤儿证据，未登记）")
            continue
        row = rows[attr]
        status = row["status"]
        if rec["status"] == "PASS" and "✅" not in status:
            rep.error("S3", f"STATUS.md:{row['line_no']} {attr}：最新 Kani 证据为 PASS（{rec['path'].name}），"
                            f"主状态「{status}」未同步（M1）")
        if rec["status"] == "FAIL" and "✅" in status:
            rep.error("S3", f"STATUS.md:{row['line_no']} {attr}：最新 Kani 证据为 FAIL（{rec['path'].name}），"
                            f"主状态却为 ✅（M1）")
        # 行内须提及该 proof（全名或尾段——P0-3 行存在「_nested_dot」式缩写）
        seg = harness.rsplit("_", 1)[-1]
        if harness not in row["text"] and not (len(seg) >= 3 and seg in row["text"]):
            rep.error("S3", f"STATUS.md:{row['line_no']} {attr}：行内未提及 proof「{harness}」"
                            f"（证据 {rec['path'].name} 已归档，须登记）")

    # 差分证据：最新 PASS → 行主状态应含 🔵（或 ✅）；最新 FAIL → 不得为 ✅
    for attr, rec in sorted(diff_groups.items()):
        for a in expand_attrs(attr):
            if a not in rows:
                rep.error("S3", f"证据 {rec['path'].name}：属性 {a} 在 STATUS.md 中无对应行（孤儿证据，未登记）")
                continue
            row = rows[a]
            status = row["status"]
            if rec["status"] == "PASS" and not ("🔵" in status or "✅" in status):
                rep.error("S3", f"STATUS.md:{row['line_no']} {a}：最新差分证据为 PASS（{rec['path'].name}），"
                                f"主状态「{status}」未同步（M1）")
            if rec["status"] == "FAIL" and "✅" in status:
                rep.error("S3", f"STATUS.md:{row['line_no']} {a}：最新差分证据为 FAIL（{rec['path'].name}），"
                                f"主状态却为 ✅（M1）")


def rule_s4(ev_files, rep):
    """S4 证据命名与配对（M3.1/M3.3）。ev_files: [(Path, rel)]。"""
    log_stems = {}
    stdout_stems = {}
    for path, rel in ev_files:
        name = path.name
        if not EV_NAME_RE.match(name):
            rep.error("S4", f"{rel}：文件名不符合 M3.1 命名规范"
                            f"（<属性号>[.<harness>]_<PASS|FAIL>_<sha>_<ts>.log）")
            continue
        if path.stat().st_size == 0:
            rep.error("S4", f"{rel}：0KB 空文件不是证据（M3.3）")
        if name.endswith(".log"):
            log_stems.setdefault(name[:-len(".log")], rel)
        else:
            stdout_stems.setdefault(name[:-len(".stdout.txt")], rel)
    for stem, rel in sorted(log_stems.items()):
        if stem not in stdout_stems:
            rep.error("S4", f"{rel}：缺少同名 .stdout.txt 伴随文件（evidence 规范 §三）")
    for stem, rel in sorted(stdout_stems.items()):
        if stem not in log_stems:
            rep.error("S4", f"{rel}：缺少同名 .log 结论文件（evidence 规范 §三）")


def rule_s5(ev_index, rep):
    """S5 FAIL 证据强制字段（M3.2）：harness/测试名 + 完整复现命令。"""
    for rec in ev_index:
        if rec["kind"] != "log" or rec["status"] != "FAIL":
            continue
        try:
            text = rec["path"].read_text(encoding="utf-8", errors="replace")
        except OSError as e:
            rep.error("S5", f"{rec['rel']}：无法读取（{e}）")
            continue
        if not re.search(r"command:.*cargo", text):
            rep.error("S5", f"{rec['rel']}：FAIL 证据缺少含 cargo 的完整复现命令（M3.2）")
        ident = rec["harness"] or rec["attr"]
        if ident not in text:
            rep.error("S5", f"{rec['rel']}：FAIL 证据内容未含 harness/测试名「{ident}」（M3.2）")


def rule_s6(rep):
    """S6 隔离区 README（M3.5）。"""
    for root in sorted(REPO_ROOT.glob("evorule-*/verification/evidence")):
        for inv in sorted(root.rglob("_invalidated")):
            if not inv.is_dir():
                continue
            if not (inv / "README.md").exists():
                rel = inv.relative_to(REPO_ROOT).as_posix()
                rep.error("S6", f"{rel}/：缺少 README.md（M3.5 隔离区须说明作废原因与隔离日期）")


def rule_s7(ev_index, git, rep):
    """S7 证据时效（M3.4）：PASS 证据基线不早于 proof 函数最后变更。"""
    for rec in sorted(ev_index, key=lambda r: r["rel"]):
        if rec["kind"] != "log" or rec["status"] != "PASS" or not rec["harness"]:
            continue  # 仅查 Kani PASS（FAIL 为过程记录，由行备注披露；差分无 proof 函数）
        if "evorule-reactor" in rec["rel"]:
            proof_file = REACTOR_PROOF_SRC
        elif "evorule-tcb" in rec["rel"]:
            proof_file = TCB_PROOF_SRC
        else:
            continue
        ev_sha = rec["sha"]
        if not git.commit_exists(ev_sha):
            rep.error("S7", f"{rec['path'].name}：证据 SHA {ev_sha} 无法在本仓解析为 commit")
            continue
        func_last = git.func_last_change(proof_file, rec["harness"])
        level = "函数级"
        if func_last is None:
            func_last = git.file_last_change(proof_file)
            level = "文件级（git -L 未命中，降级）"
        if func_last is None:
            rep.note("S7", f"{rec['path'].name}：无法确定 proof 变更历史，跳过时效判定")
            continue
        if not git.is_ancestor(func_last, ev_sha):
            rep.error("S7", f"{rec['path'].name}：证据基线 {ev_sha[:7]} 早于 proof「{rec['harness']}」最后变更 "
                            f"{func_last[:7]}（{level}）——按 M3.4 旧证据应隔离或重跑")


def rule_s8(known_tcb, known_reactor, a_names, b_names, ci_names, nonci_names,
            a_decl, b_decl, ci_decl, nonci_decl, yml_jobs, cargo_meta, rep):
    """S8 CI 闸门与清单对齐（M1.3/M8）。"""
    known_all = known_tcb | known_reactor

    def job_proofs(job):
        body = yml_jobs.get(job, "")
        return set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", body)) & known_all

    yml_a = job_proofs("kani-tcb-a-tier")
    yml_b = job_proofs("kani-tcb-b-tier")
    yml_reactor = job_proofs("kani-reactor")

    for label, declared, names in (
        ("附录 B A 档", a_decl, a_names), ("附录 B B 档", b_decl, b_names),
        ("附录 C 入闸", ci_decl, ci_names), ("附录 C 未入 CI", nonci_decl, nonci_names),
    ):
        if declared != len(names):
            rep.error("S8", f"STATUS.md {label}：标题声明 {declared} 个，清单实际 {len(names)} 个")

    if yml_a != a_names:
        rep.error("S8", f"kani.yml kani-tcb-a-tier 与 STATUS.md 附录 B A 档不一致："
                        f"仅 yml 有 {sorted(yml_a - a_names)}，仅附录有 {sorted(a_names - yml_a)}")
    if yml_b != b_names:
        rep.error("S8", f"kani.yml kani-tcb-b-tier 与 STATUS.md 附录 B B 档不一致："
                        f"仅 yml 有 {sorted(yml_b - b_names)}，仅附录有 {sorted(b_names - yml_b)}")
    if yml_reactor != ci_names:
        rep.error("S8", f"kani.yml kani-reactor 与 STATUS.md 附录 C 入闸清单不一致："
                        f"仅 yml 有 {sorted(yml_reactor - ci_names)}，仅附录有 {sorted(ci_names - yml_reactor)}")
    for crate, proofs in sorted(cargo_meta.items()):
        if crate == "evorule-reactor" and proofs != ci_names:
            rep.error("S8", f"{crate}/Cargo.toml [package.metadata.kani] 与附录 C 入闸清单不一致："
                            f"仅 Cargo 有 {sorted(proofs - ci_names)}，仅附录有 {sorted(ci_names - proofs)}")
    unregistered = known_tcb - (a_names | b_names)
    if unregistered:
        rep.error("S8", f"TCB proof 源码有 {len(unregistered)} 个函数未登记入 STATUS.md 附录 B：{sorted(unregistered)}")
    unregistered = known_reactor - (ci_names | nonci_names)
    if unregistered:
        rep.error("S8", f"reactor proof 源码有 {len(unregistered)} 个函数未登记入 STATUS.md 附录 C：{sorted(unregistered)}")
    stale = (a_names | b_names) - known_tcb
    if stale:
        rep.error("S8", f"STATUS.md 附录 B 清单中有 {len(stale)} 个 proof 在源码中不存在：{sorted(stale)}")
    stale = (ci_names | nonci_names) - known_reactor
    if stale:
        rep.error("S8", f"STATUS.md 附录 C 清单中有 {len(stale)} 个 proof 在源码中不存在：{sorted(stale)}")


# S9 对外数字锚点：(文件, regex, 捕获组对应的推导键, 是否必须存在)
S9_ANCHORS = [
    ("README.md", re.compile(r"Kani-(\d+)%20proofs%20%28(\d+)%20verified%29"), ("total", "verified"), True),
    ("README.md", re.compile(r"Verified \(current re-run\)\*?\*?:\s*(\d+)"), ("verified",), False),
    ("README.md", re.compile(r"当前实跑验证\*\*：\s*(\d+)\s*个"), ("verified",), False),
    ("README.md", re.compile(r"(\d+) 个 proof 中 (\d+) 个当前实跑验证"), ("total", "verified"), False),
    ("README.md", re.compile(r"B 档 (\d+) 个判定当前不可运行"), ("b",), False),
    ("README.md", re.compile(r"共 \*\*(\d+) 个\*\*（tcb (\d+) \+ reactor (\d+)）"), ("total", "tcb", "reactor"), False),
    ("README.md", re.compile(r"（(\d+) 个 = A 档 (\d+) \+ B 档 (\d+)"), ("tcb", "a", "b"), False),
    ("ROADMAP.md", re.compile(r"A 档 (\d+) proof ✅、B 档 (\d+) proof ❌"), ("a", "b"), False),
    ("verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md", re.compile(r"（(\d+) 个 proof，源码"), ("reactor",), False),
    ("verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md", re.compile(r"其中 (\d+) 个入 kani\.yml PR 闸门"), ("rc_ci",), False),
    ("verification/README.md", re.compile(r"TCB A 档 (\d+) \+ reactor (\d+) 个"), ("a", "rc_ci"), False),
    ("verification/README.md", re.compile(r"B 档 (\d+) 个（仅手动触发"), ("b",), False),
    ("evorule-reactor/docs/KANI.md", re.compile(r"CI 子集（(\d+) 个"), ("rc_ci",), False),
    ("evorule-reactor/docs/KANI.md", re.compile(r"完整验证（(\d+) 个"), ("rc_nonci",), False),
    ("evorule-reactor/docs/KANI.md", re.compile(r"\*\*(\d+) 个 CI proof\*\*"), ("rc_ci",), False),
]


def rule_s9(derived, ev_index, rep):
    """S9 对外数字对齐（M1/M6）。"""
    cache = {}

    def file_lines(rel):
        if rel not in cache:
            path = REPO_ROOT / rel
            cache[rel] = (path.read_text(encoding="utf-8", errors="replace").splitlines()
                          if path.exists() else None)
        return cache[rel]

    for rel, pattern, keys, required in S9_ANCHORS:
        lines = file_lines(rel)
        if lines is None:
            if required:
                rep.error("S9", f"{rel}：文件缺失（含必须的对外数字声明）")
            continue
        found = False
        for i, line in enumerate(lines, 1):
            for m in pattern.finditer(line):
                found = True
                for val, key in zip(m.groups(), keys):
                    if int(val) != derived[key]:
                        rep.error("S9", f"{rel}:{i}：声明 {key} = {val}，与 STATUS.md 附录推导值 "
                                        f"{derived[key]} 不一致（M1 单一真相源）")
        if required and not found:
            rep.error("S9", f"{rel}：未找到必须的对外数字声明（Kani badge）")

    # 第三源交叉验证：证据库最新为 PASS 的 Kani proof 去重数 = 「当前实跑验证」
    latest = {}
    for rec in ev_index:
        if rec["kind"] != "log" or not rec["harness"]:
            continue
        h = rec["harness"]
        if h not in latest or rec["ts"] >= latest[h]["ts"]:
            latest[h] = rec
    pass_count = sum(1 for r in latest.values() if r["status"] == "PASS")
    if pass_count != derived["verified"]:
        rep.error("S9", f"证据库最新为 PASS 的 Kani proof 共 {pass_count} 个，与「当前实跑验证」推导值 "
                        f"{derived['verified']} 不一致（入闸 proof 须归档证据，M2）")


def rule_s10(status_text, mech_text, cargo_version, rep):
    """S10 版本对齐（M4）。"""
    ms = re.search(r"快照\*{0,2}：v(\d+\.\d+\.\d+)", status_text)
    mm = re.search(r'workspace\s*`version\s*=\s*"(\d+\.\d+\.\d+)"`', mech_text or "")
    if not ms:
        rep.error("S10", "STATUS.md 头部缺少「快照：v…」版本声明")
        return
    v_status = ms.group(1)
    if cargo_version is None:
        rep.error("S10", "Cargo.toml 未找到 workspace version（M4）")
    elif cargo_version != v_status:
        rep.error("S10", f"STATUS.md 快照 v{v_status} ≠ Cargo.toml workspace version {cargo_version}（M4）")
    if mech_text is None:
        rep.note("S10", "MECHANISM.md 缺失，跳过其版本对齐声明检查")
    elif mm is None:
        rep.error("S10", "MECHANISM.md 头部缺少版本对齐声明（M4）")
    elif mm.group(1) != v_status:
        rep.error("S10", f"MECHANISM.md 版本对齐声明 v{mm.group(1)} ≠ STATUS.md 快照 v{v_status}（M4）")


def rule_s11(git, rep):
    """S11 披露留痕联动（M5）。"""
    status_lc = git.last_change("verification/STATUS.md")
    disc_lc = git.last_change("verification/DISCLOSURE_LOG.md")
    if status_lc is None:
        rep.note("S11", "verification/STATUS.md 无 git 历史（新文件？），跳过已提交留痕比对")
    elif disc_lc is None:
        rep.error("S11", "verification/STATUS.md 有提交历史，但 DISCLOSURE_LOG.md 无提交历史（M5 留痕缺失）")
    elif disc_lc[1] < status_lc[1]:
        rep.error("S11", f"STATUS.md 最后变更（{status_lc[0][:7]}）晚于 DISCLOSURE_LOG.md 最后变更"
                         f"（{disc_lc[0][:7]}）——状态变更未留披露（M5）")
    # 工作区：未提交变更须成对出现（改了 STATUS 忘了写披露 → 提交前拦下）
    status_dirty = git.porcelain("verification/STATUS.md")
    disc_dirty = git.porcelain("verification/DISCLOSURE_LOG.md")
    if status_dirty and not disc_dirty:
        rep.error("S11", "工作区中 STATUS.md 有未提交变更，而 DISCLOSURE_LOG.md 无对应变更——"
                         "补留痕条目后一并提交（M5）")


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

RULE_LABELS = [
    ("S1", "状态词汇合规（M2）"),
    ("S2", "状态→证据齐备（M2/M3.1）"),
    ("S3", "证据→状态闭环（M1）"),
    ("S4", "证据命名与配对（M3.1/M3.3）"),
    ("S5", "FAIL 证据强制字段（M3.2）"),
    ("S6", "隔离区 README（M3.5）"),
    ("S7", "证据时效（M3.4）"),
    ("S8", "CI 闸门与清单对齐（M1.3/M8）"),
    ("S9", "对外数字对齐（M1/M6）"),
    ("S10", "版本对齐（M4）"),
    ("S11", "披露留痕联动（M5）"),
]


def main():
    global REPO_ROOT
    parser = argparse.ArgumentParser(description="EvoRule 验证状态同步检查器（S 系规则）")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--strict", action="store_true", default=True, help="违规时 exit 1（默认）")
    mode.add_argument("--warn", action="store_true", help="仅输出违规，exit 总是 0")
    parser.add_argument("--skip-git", action="store_true", help="跳过 S7/S11（非 git 环境）")
    parser.add_argument("--json", action="store_true", help="JSON 输出（CI 消费）")
    parser.add_argument("--repo", default=None, help="仓库根目录（默认：脚本所在 scripts/ 的上级；仓外运行时指定）")
    args = parser.parse_args()

    if args.repo:
        REPO_ROOT = Path(args.repo).resolve()

    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(encoding="utf-8", errors="replace")
            except Exception:
                pass

    rep = Report()
    try:
        status_path = REPO_ROOT / "verification" / "STATUS.md"
        mech_path = REPO_ROOT / "verification" / "MECHANISM.md"
        cargo_path = REPO_ROOT / "Cargo.toml"
        yml_path = REPO_ROOT / ".github" / "workflows" / "kani.yml"
        if not status_path.exists():
            raise EnvError(f"缺失：{status_path}")
        status_text = status_path.read_text(encoding="utf-8", errors="replace")
        mech_text = mech_path.read_text(encoding="utf-8", errors="replace") if mech_path.exists() else None

        rows, sections = parse_status(status_text)
        if not rows:
            raise EnvError("STATUS.md 未解析到任何属性行（P0/P1 表）")

        a_decl, a_names, b_decl, b_names = parse_appendix_b(sections["appB"])
        ci_decl, ci_names, nonci_decl, nonci_names = parse_appendix_c(sections["appC"])

        ev_files = list(iter_evidence_files())
        ev_index, log_basenames = build_evidence_index()

        known_tcb = parse_proof_source(TCB_PROOF_SRC)
        known_reactor = parse_proof_source(REACTOR_PROOF_SRC)
        yml_jobs = parse_kani_yml_jobs(yml_path)
        if "kani-tcb-a-tier" not in yml_jobs or "kani-reactor" not in yml_jobs:
            raise EnvError("kani.yml 缺少 kani-tcb-a-tier / kani-reactor job")
        cargo_meta = parse_cargo_kani_proofs()

        git = Git(enabled=not args.skip_git)

        # ---- 规则执行 ----
        rule_s1(rows, rep)
        rule_s2(rows, log_basenames, rep)
        rule_s3(rows, ev_index, rep)
        rule_s4(ev_files, rep)
        rule_s5(ev_index, rep)
        rule_s6(rep)
        if args.skip_git:
            rep.note("S7", "--skip-git：跳过")
            rep.note("S11", "--skip-git：跳过")
        else:
            rule_s7(ev_index, git, rep)
            rule_s11(git, rep)
        rule_s8(known_tcb, known_reactor, a_names, b_names, ci_names, nonci_names,
               a_decl, b_decl, ci_decl, nonci_decl, yml_jobs, cargo_meta, rep)
        derived = {
            "a": len(a_names), "b": len(b_names),
            "rc_ci": len(ci_names), "rc_nonci": len(nonci_names),
            "tcb": len(a_names) + len(b_names),
            "reactor": len(ci_names) + len(nonci_names),
            "total": len(a_names) + len(b_names) + len(ci_names) + len(nonci_names),
            "verified": len(a_names) + len(ci_names),
        }
        rule_s9(derived, ev_index, rep)
        rule_s10(status_text, mech_text,
                 cargo_workspace_version(cargo_path) if cargo_path.exists() else None, rep)
    except EnvError as e:
        if args.json:
            print(json.dumps({"rc": 2, "error": str(e)}, ensure_ascii=False))
        else:
            print(f"[ENV] {e}", file=sys.stderr)
        return 2

    errors = rep.errors()
    rc = 1 if (errors and not args.warn) else 0

    if args.json:
        def state(rid):
            if any(i["rule"] == rid and i["severity"] == "error" for i in rep.items):
                return "FAIL"
            if any(i["rule"] == rid and "跳过" in i["message"] for i in rep.items):
                return "SKIP"
            return "PASS"
        print(json.dumps({
            "rc": rc,
            "summary": {rid: state(rid) for rid, _ in RULE_LABELS},
            "violations": [i for i in rep.items if i["severity"] == "error"],
            "notes": [i for i in rep.items if i["severity"] == "note"],
        }, ensure_ascii=False, indent=2))
        return rc

    print("== check_status_sync.py（验证状态同步检查）==")
    for rid, label in RULE_LABELS:
        errs = [i for i in rep.items if i["rule"] == rid and i["severity"] == "error"]
        notes = [i for i in rep.items if i["rule"] == rid and i["severity"] == "note"]
        skipped = any("跳过" in n["message"] for n in notes)
        if errs:
            print(f"[{rid}] {label} ................ FAIL")
            for i in errs:
                print(f"    - {i['message']}")
        elif skipped:
            print(f"[{rid}] {label} ................ SKIP")
        else:
            print(f"[{rid}] {label} ................ PASS")
        for n in notes:
            if "跳过" not in n["message"]:
                print(f"    * {n['message']}")
    n_fail = sum(1 for rid, _ in RULE_LABELS
                 if any(i["rule"] == rid and i["severity"] == "error" for i in rep.items))
    print(f"\n汇总：{len(RULE_LABELS)} 项规则，{len(RULE_LABELS) - n_fail} PASS / {n_fail} FAIL")
    print(f"RC={rc}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
