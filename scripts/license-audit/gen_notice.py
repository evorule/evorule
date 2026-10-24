#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Render D:\\evorule\\THIRD-PARTY-NOTICES.md from the audit inventory."""
import json
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(r"D:\evorule")
OUT = ROOT / "THIRD-PARTY-NOTICES.md"

REPO_META = {
    "evorule": ("核心引擎 / TCB 唯一版本源（8 crate）",
                "crates.io ×6 + CLI 双平台二进制", "cargo"),
    "evorule-server": ("官方 HTTP 服务入口（:18080）",
                       "零依赖分发包 win64.zip / linux64.tar.gz", "cargo + npm"),
    "evorule-rule": ("规则资产治理层",
                     "crates.io + 双平台双 bin", "cargo"),
    "evorule-system-rules": ("生态宪法 / schema 版本权威",
                              "仓库内资产（schemas/migrations/signatures）", "无编译依赖"),
    "evorule-bundle": ("治理↔执行唯一传递契约",
                       "crates.io", "cargo"),
    "evorule-hash": ("审计链规范哈希 SSOT",
                     "crates.io", "cargo"),
    "evorule-console-cloud": ("云控制台 / 合规审计层",
                              "静态站点 zip + GitHub Pages", "npm"),
    "evorule-integration-tests": ("跨仓一致性门禁",
                                  "不分发", "cargo"),
    "evorule-context-inspector": ("上下文检查器",
                                   "crates.io", "cargo"),
    "evorule-wasm-demo": ("WASM 演示",
                          "npm 包 + wasm 产物", "cargo + npm"),
    "evo-agent": ("应用层编排（可信 AI 工作站）",
                  "npm 包 + 桌面端", "cargo + npm + pypi"),
    "evo-agent-desktop": ("桌面壳发行 / 自动更新通道",
                           "桌面安装包", "pypi / 脚本"),
    "sync-sentinel": ("双端一致性哨兵",
                      "不分发（运维脚本）", "shell"),
}

OBLIG = {
    "MIT": "保留版权声明与许可证全文，随二进制分发。",
    "MIT-0": "保留版权声明与许可证全文，随二进制分发。",
    "Apache-2.0": "保留版权声明、许可证全文与变更说明；上游若有 NOTICE 须一并附带（§4(d)）。",
    "BSD-2-Clause": "保留版权声明、条款与免责声明。",
    "BSD-3-Clause": "保留版权声明、条款与免责声明。",
    "ISC": "保留版权声明与许可文本。",
    "Zlib": "保留版权声明与许可文本。",
    "0BSD": "无附加义务。",
    "Unlicense": "无义务。",
    "Unicode-3.0": "保留版权声明与许可文本。",
    "CC0-1.0": "公共领域贡献，无义务。",
    "MPL-2.0": "文件级 copyleft：被修改的 MPL 文件须以 MPL 公开；分发时保留声明与正文。",
    "AGPL-3.0-or-later": "网络 copyleft：以网络提供服务时须向使用者提供对应源码（§13）。",
    "BSL-1.0": "宽松许可：保留版权声明与正文。",
    "BlueOak-1.0.0": "宽松但非 OSI 认证：保留版权声明与正文。",
    "CDLA-Permissive-2.0": "宽松许可：保留版权声明与正文。",
    "NCSA": "宽松许可：保留版权声明与正文。",
    "OFL-1.1": "字体许可：保留声明、不得单独售卖字体、须检查保留字体名条款。",
    "LGPL-2.1-or-later": "弱 copyleft：本生态统一选择 MIT/Apache 分支，不触发 LGPL 条款。",
}

SPECIAL = [
    ("MPL-2.0", "dompurify（evo-agent 前端）",
     "MPL-2.0 为文件级 copyleft。dompurify 的表达式为 `MPL-2.0 OR Apache-2.0`，"
     "可选择 Apache-2.0 分支规避 copyleft；但一旦修改其源码并分发该修改文件，"
     "须以 MPL-2.0 公开该文件。当前为原样依赖，义务为零。"),
    ("BlueOak-1.0.0", "lru-cache（evo-agent）",
     "BlueOak-1.0.0 是宽松许可但**未经 OSI 认证**。法律上可用，但若下游客户"
     "要求「全部依赖为 OSI 认证许可」，此项会成为审计问询点。已列入 deny.toml 白名单并附正文。"),
    ("CDLA-Permissive-2.0", "webpki-roots / webpki-root-certs（evorule-server、evo-agent）",
     "根证书库采用 CDLA-Permissive-2.0。宽松许可，仅要求保留声明。"
     "注意：根证书内容本身由各 CA 分发，其内容的独立授权不由本仓库承担。"),
    ("NCSA", "libfuzzer-sys（evo-agent，仅 dev 依赖）",
     "表达式为 `(MIT OR Apache-2.0) AND NCSA`——NCSA 部分是 **AND 强制项**，"
     "不可通过 OR 选择规避，须随附 NCSA 正文。"),
    ("BSL-1.0", "clipboard-win / error-code（evo-agent）",
     "Boost Software License 1.0，宽松，保留声明即可。"),
    ("MIT-0", "borrow-or-share（evorule-server）、@csstools/*（evo-agent、console-cloud）",
     "MIT-0 是 MIT 的零归属变体，义务低于 MIT。"),
    ("LGPL-2.1-or-later", "r-efi 等 11 项（多仓）",
     "表达式均为 `MIT OR Apache-2.0 OR LGPL-2.1-or-later`。本生态**统一选择 MIT/Apache 分支**，"
     "因此不触发 LGPL 的动态链接与可替换性要求；已附 LGPL 正文以备下游自行选择。"),
]

DIST_MATRIX = [
    ("evorule", "crates.io 包（6 个 crate）",
     "✅ 已随包（LICENSE + NOTICE.md）",
     "⚠️ evorule-discipline / evorule-eval 两包**缺 LICENSE 正文**"),
    ("evorule-server", "零依赖分发包 win64.zip / linux64.tar.gz",
     "❌ 包内无 LICENSE、无 NOTICE、无第三方声明、无源码提供入口",
     "必须补：LICENSE + NOTICE + THIRD-PARTY-NOTICES + 源码提供链接（AGPL §6/§13）"),
    ("evorule-rule", "双平台双 bin",
     "❌ 包内无 LICENSE", "必须补：LICENSE + NOTICE + THIRD-PARTY-NOTICES"),
    ("evorule-bundle", "crates.io 包", "✅ 已随包（LICENSE + 双许可文件 + NOTICE）", "—"),
    ("evorule-hash", "crates.io 包", "✅ 已随包（LICENSE + 双许可文件 + NOTICE）", "—"),
    ("evorule-console-cloud", "静态站点 zip + GitHub Pages",
     "❌ 包内无 LICENSE / NOTICE", "必须补（前端 npm 依赖归属）+ §13 源码提供"),
    ("evorule-wasm-demo", "npm 包 evorule-wasm-demo",
     "✅ npm 包内含 LICENSE", "npm 依赖的第三方声明未随包"),
    ("evo-agent", "npm 包 evo-agent-node / 桌面端",
     "❌ sdk/nodejs 目录**无 LICENSE**（npm 发布包将不含 AGPL 正文）",
     "必须补：LICENSE + THIRD-PARTY-NOTICES 进 npm 包"),
    ("evo-agent-desktop", "桌面安装包",
     "❌ 无 LICENSE / 第三方声明", "必须补（Tauri 打包需携带）"),
    ("evorule-system-rules", "仓库资产", "✅ 仓根有 LICENSE", "—"),
    ("evorule-context-inspector", "crates.io 包", "✅ 已随包", "—"),
    ("evorule-integration-tests", "不分发", "—", "—"),
    ("sync-sentinel", "不分发", "—", "—"),
]


def main():
    inv = json.loads(Path(r"D:\minimax-agent\.audit\inv_final.json").read_text(encoding="utf-8"))
    man = json.loads((ROOT / "LICENSES" / "third-party" / "manifest.json").read_text(encoding="utf-8"))

    per_repo = {}
    totals = Counter()
    lic_crates = defaultdict(set)
    for repo, r in inv.items():
        c = npm = py = 0
        for x in r["crates"]:
            if x["license"] in ("(local path/workspace)", "(git dependency)"):
                continue
            c += 1
            totals[x["license"]] += 1
            lic_crates[x["license"]].add(x["name"])
        for n in r["npm"]:
            npm += 1
            totals[n["license"]] += 1
            lic_crates[n["license"]].add(n["name"])
        for p in r["python"]:
            py += 1
            totals[p["license"] or p.get("classifier", "")] += 1
        per_repo[repo] = (c, npm, py)

    total_deps = sum(totals.values())

    # licence-id aggregation (composite expressions -> their ids)
    def ids(expr):
        known = list(OBLIG.keys()) + ["Apache-2.0 WITH LLVM-exception",
                                      "BSD-1-Clause", "PostgreSQL", "X11",
                                      "CC-BY-4.0", "LGPL-2.1", "EPL-2.0"]
        import re
        out = []
        for k in sorted(known, key=len, reverse=True):
            if re.search(r"(?<![\w.+-])" + re.escape(k) + r"(?![\w.+-])", expr or ""):
                out.append(k)
        return out

    lic_agg = defaultdict(lambda: {"count": 0, "crates": set()})
    for expr, n in totals.items():
        for i in ids(expr) or [expr]:
            lic_agg[i]["count"] += n
            lic_agg[i]["crates"] |= lic_crates.get(expr, set())

    L = []
    A = L.append
    A("# THIRD-PARTY-NOTICES — EvoRule 生态第三方组件使用声明")
    A("")
    A("> **本文件是 EvoRule 生态 13 个在役仓库的第三方组件使用声明（使用声明文件）。**")
    A("> 适用范围、义务边界、归档位置与复现方法见下文。")
    A("> 首次生成：2026-10-24 · 审计基线：13 在役仓 · 依赖记录 %d 条" % total_deps)
    A("")
    A("---")
    A("")
    A("## 〇、一句话结论")
    A("")
    A("**依赖许可证面无禁止级风险（GPL-2.0-only / SSPL / BUSL / 非商用限制 = 0 项）；"
      "但分发义务未履行——各仓发行产物未随附许可证正文、第三方声明与 AGPL 源码提供入口，"
      "这是当前最实际的法律风险点。**")
    A("")
    A("| 维度 | 结论 |")
    A("|---|---|")
    A("| 禁止级许可证（copyleft 不兼容 / 源码可见 / 非商用） | **0 项** |")
    A("| 需登记的宽松许可（不在原白名单） | 7 类（MIT-0 / CDLA-Permissive-2.0 / "
      "BSL-1.0 / BlueOak-1.0.0 / NCSA / OFL-1.1 / BSD-1-Clause）— "
      "**已于 2026-10-24 全部登记入 `deny.toml`**，复验 0 红项 |")
    A("| 需特别标注的 copyleft 依赖 | 1 类（MPL-2.0，dompurify，可选 Apache-2.0 分支） |")
    A("| 已发布 crate 缺许可证正文 | **2 / 9**（evorule-discipline、evorule-eval） |")
    A("| 发行分发包缺 LICENSE/NOTICE/声明 | **6 / 13** 仓 |")
    A("| 依赖许可 CI 门禁覆盖 | **1 / 13** 仓（仅根仓有 cargo-deny） |")
    A("| 自有许可七件套一致性 | **红**：17 项不一致（10 仓），详见第二节 |")
    A("")
    A("---")
    A("")
    A("## 一、适用范围：13 个在役仓")
    A("")
    A("| # | 仓名 | 角色 | 依赖生态 | 依赖记录数 | 分发形态 |")
    A("|---|---|---|---|---|---|")
    for i, (repo, (role, ships, eco)) in enumerate(REPO_META.items(), 1):
        c, npm, py = per_repo.get(repo, (0, 0, 0))
        n = c + npm + py
        A("| %d | `%s` | %s | %s | %d | %s |" % (i, repo, role, eco, n, ships))
    A("| | **合计** | | | **%d** | |" % total_deps)
    A("")
    A("---")
    A("")
    A("## 二、本生态自身的许可与双许可路径")
    A("")
    A("| 资产 | 许可 | 义务要点 |")
    A("|---|---|---|")
    A("| 代码（全部 13 仓） | AGPL-3.0-or-later | 分发须附许可证副本与修改说明（§4/§5(d)）；"
      "以网络提供服务须提供对应源码（§13） |")
    A("| `core_eval.json`（宪法/解释器规范） | CC0-1.0 公共领域 | 无义务，鼓励第三方实现兼容引擎 |")
    A("| 贡献物通道 D | 局部 Apache-2.0 再授权 | 见 `CONTRIBUTION_GRANT.md` |")
    A("")
    A("**商业许可路径的差异**：持有商业许可（`COMMERCIAL_LICENSE.md`）的用户，其使用与分发"
      "不再受 AGPL 源码提供与 copyleft 传染约束；第三方组件的归属义务（MIT/BSD/Apache 的"
      "版权声明与正文）**不因商业许可而免除**——本文件所列第三方义务对两条路径同样成立。")
    A("")
    A("### 与既有「许可文件七件套」的关系")
    A("")
    A("生态已有一套自有许可治理机制：根仓 `scripts/sync-licenses.ps1` 以主仓为权威源，"
      "把 **CLA-corporate / CLA-individual / COMMERCIAL_LICENSE / DUAL_LICENSE / "
      "FREE_COMMERCIAL_LICENSE / LICENSE / NOTICE.md** 七件套同步到各副本仓并做一致性核验；"
      "`scripts/validate-license.ps1` 校验 LICENSE 存在性、AGPL/CC0 标识、`.rs` SPDX 头与 SDK 许可字段。")
    A("")
    A("两者是**不同层面，互不替代**：")
    A("")
    A("| 层面 | 管什么 | 载体 |")
    A("|---|---|---|")
    A("| 自有许可层（七件套） | 本生态代码的授权路径、CLA、双许可文本在各仓的一致性 | 各仓根 7 个文件 |")
    A("| 第三方归属层（本文件） | 依赖的第三方组件的许可、归属义务与正文 | 根仓本文件 + `LICENSES/third-party/` |")
    A("")
    A("本文件**不纳入七件套**：第三方依赖逐仓不同，七件套的「各仓内容一致」语义不适用于此；"
      "它是生态级（跨 13 仓）的汇总声明。若需单仓声明，可从 "
      "`LICENSES/third-party/manifest.json` 的 `dependencies[].repo` 字段按仓筛选取子集生成。")
    A("")
    A("**七件套的两个现存缺口**（2026-10-24 实测 `sync-licenses.ps1 -Check`）：")
    A("")
    A("1. **门禁当前为红**：17 项不一致（`NOTICE.md` × 10 仓、`DUAL_LICENSE.md` × 7 仓）。"
      "副本仓停在 2026-09-08「通道 D 贡献授权」条款加入之前的旧版，属真实内容漂移，"
      "不是仓名归一化能吸收的差异。修复：`powershell -File scripts/sync-licenses.ps1 -Sync`。")
    A("2. **覆盖不全**：七件套核验的 11 仓里，4 仓（`evorule-console` / `evorule-dsh-skill` / "
      "`evorule-experience-pack` / `evorule-sdk`）已不在 13 在役公开仓内（均为私有仓），"
      "而 5 个在役仓（`evorule-integration-tests` / `evorule-context-inspector` / "
      "`evorule-wasm-demo` / `evo-agent-desktop` / `sync-sentinel`）**未被纳入核验**。")
    A("")
    A("---")
    A("")
    A("## 三、第三方许可总览")
    A("")
    A("下表按许可证标识聚合。复合表达式（如 `MIT OR Apache-2.0`）按其可选分支拆分统计，"
      "同一依赖可能计入多行。")
    A("")
    A("| 许可证 | 依赖记录数 | 涉及组件（示例） | 义务 | 正文归档 |")
    A("|---|---|---|---|---|")
    order = sorted(lic_agg.items(), key=lambda kv: -kv[1]["count"])
    for lic, info in order:
        if lic in ("NO-LICENSE-FIELD", "UNLICENSED/UNDECLARED", "NOT-FOUND",
                   "UNRESOLVED", ""):
            continue
        sample = sorted(info["crates"])[:4]
        samp = ", ".join("`%s`" % s for s in sample) or "—"
        more = " 等" if len(info["crates"]) > 4 else ""
        obl = OBLIG.get(lic, "保留版权声明与许可正文。")
        arch = "✅" if lic in man.get("licenses", {}) and \
            man["licenses"][lic].get("file") else "⚠️ 需人工补"
        A("| `%s` | %d | %s%s | %s | %s |" % (lic, info["count"], samp, more, obl, arch))
    A("")
    A("机器可读全量清单见 [`LICENSES/third-party/manifest.json`](LICENSES/third-party/manifest.json)"
      "（%d 条依赖记录，含每条的许可标识与来源）。" % total_deps)
    A("")
    A("---")
    A("")
    A("## 四、需特别标注的组件")
    A("")
    for lic, who, why in SPECIAL:
        A("### `%s` — %s" % (lic, who))
        A("")
        A(why)
        A("")
    A("---")
    A("")
    A("## 五、分发义务矩阵（这是当前的主要缺口）")
    A("")
    A("AGPL-3.0 的分发义务：§4（随附许可证副本 + 保留声明 + 修改版加「Appropriate Legal "
      "Notices」）、§6（分发对象代码须附 §4 要求的通知）、§13（网络服务须提供对应源码）。"
      "同时 MIT/BSD/ISC/Apache 等宽松许可要求**随分发物保留其版权声明与许可正文**。")
    A("")
    A("| 仓 | 分发物 | 现状 | 结论 |")
    A("|---|---|---|---|")
    for repo, artifact, status, verdict in DIST_MATRIX:
        A("| `%s` | %s | %s | %s |" % (repo, artifact, status, verdict))
    A("")
    A("> 上表「现状」为 2026-10-24 对各仓 `release.yml` / `ci.yml` 打包步骤的逐行核验，"
      "并对 crates.io 上 9 个已发布 crate 的 `.crate` 包内容做了下载实测。")
    A("")
    A("---")
    A("")
    A("## 六、声明文件的放置规范")
    A("")
    A("| 文件 | 位置 | 作用 |")
    A("|---|---|---|")
    A("| 本文件 | 根仓 `THIRD-PARTY-NOTICES.md` | 生态级第三方使用声明（总纲） |")
    A("| 许可证正文 | 根仓 `LICENSES/third-party/<许可标识>/` | 各许可证的逐字正文，"
      "供随分发物附带的 `LICENSES/` 目录使用 |")
    A("| 机器清单 | 根仓 `LICENSES/third-party/manifest.json` | 全量依赖-许可映射（CI 可校验） |")
    A("| 自身许可 | 各仓根 `LICENSE` | AGPL-3.0-or-later 全文 |")
    A("| 自身声明 | 各仓根 `NOTICE.md` | 该仓自身的版权/许可指引 |")
    A("| 依赖门禁 | 各仓根 `deny.toml` + CI `cargo deny check` | 许可白名单门禁 |")
    A("| 审计管线 | 根仓 `scripts/license-audit/` | 本文件的生成脚本（可复现） |")
    A("")
    A("**分发时的落地做法**：每个分发包根目录放 `LICENSE`（AGPL 全文）+ `NOTICE.md` + "
      "`THIRD-PARTY-NOTICES.md`（本文件副本）+ `LICENSES/third-party/`（许可证正文目录）。"
      "AGPL §13 的源码提供入口在包的 `README` 或 `NOTICE.md` 中给出对应仓库地址与 commit。")
    A("")
    A("---")
    A("")
    A("## 七、复现与更新")
    A("")
    A("```bash")
    A("# 审计管线位于 scripts/license-audit/（随本仓版本化，可复现）")
    A("cd scripts/license-audit")
    A("")
    A("# 1. 提取各仓依赖与许可（cargo 缓存优先，crates.io 兜底）")
    A("python extract_licenses.py repos.json inv_raw.json")
    A("python resolve_licenses.py inv_raw.json inv_final.json")
    A("")
    A("# 2. SPDX 语义求值与风险分级")
    A("python spdx_eval.py inv_final.json spdx_verdicts.json")
    A("")
    A("# 3. 归档许可证正文（逐字取自 cargo 缓存 / npm registry）")
    A("python gen_third_party.py inv_final.json")
    A("python fetch_missing_licenses.py   # NCSA / LGPL-2.1：上游 crate 未分发正文")
    A("python finalize_manifest.py")
    A("")
    A("# 4. 生成本文件")
    A("python gen_notice.py")
    A("```")
    A("")
    A("**更新 SOP**：新增依赖后重跑上述流程；若引入新许可标识，需同时")
    A("①在对应仓 `deny.toml` 的 `[licenses].allow` 登记并写明理由（根仓 `deny.toml` 第 4-5 行"
      "已定此纪律），②在本文档第三节表格登记，③在 `LICENSES/third-party/` 归档正文。")
    A("")
    A("---")
    A("")
    A("## 八、待人工确认项")
    A("")
    A("1. **`evo-agent-node`（npm）发布包缺 LICENSE** — `sdk/nodejs/` 目录内无 LICENSE 文件，"
      "npm 打包时不会自动附带，发布出去的 AGPL 包将不含许可证正文。")
    A("2. **`evorule-discipline` / `evorule-eval`（crates.io）发布包缺 LICENSE** — "
      "实测 `.crate` 包内仅 5 个文件，无 LICENSE、无 NOTICE。")
    A("3. **两个 npm 工作区包未声明 license 字段** — `evo-agent-desktop`、"
      "`evorule-codegraph-bridge`（均为 private，不发布），但建议补 `AGPL-3.0-or-later` 以保持一致。")
    A("4. **`@colbymchenry/codegraph` 为 `file:` 仓外本地依赖** — 指向 `evorule-project/codegraph`"
      "（MIT，非 git 仓），不可复现、不可溯源，建议改为 version/git-rev 依赖。")
    A("5. **依赖许可 CI 门禁仅覆盖 1/13 仓** — 建议将其余 12 仓接入 cargo-deny 门禁。")
    A("6. **七件套一致性门禁当前为红**（17 项不一致，10 仓）— 见第二节；"
      "5 个在役仓尚未纳入七件套核验范围。")
    A("7. **`evorule-law-packs` 不在在役仓清单内** — 4 个判据包基于官方原文自行编写、"
      "未使用第三方译文（见各包 MAPPING.md 声明），法律风险低；但若未来随产品分发，"
      "需补其自身的许可声明（当前无 git 仓、无 LICENSE）。")
    A("")
    A("---")
    A("")
    A("*本文件由 evorule 许可证合规审计流程生成；数据来源为各仓 `Cargo.lock`、"
      "`package-lock.json`、`requirements*.txt`，许可正文逐字取自 cargo 本地 registry 缓存"
      "与 npm registry tarball（仅行尾按仓库约定统一为 LF 存储）。"
      "NCSA / LGPL-2.1 两份正文因上游 crate 不分发，另从 OSI 与 gnu.org 取，"
      "来源与理由见对应目录的 `SOURCE.txt`。*")

    text = "\n".join(L) + "\n"
    OUT.write_bytes(text.replace("\r\n", "\n").encode("utf-8"))
    print("wrote", OUT, len(text.encode("utf-8")), "bytes")


if __name__ == "__main__":
    main()
