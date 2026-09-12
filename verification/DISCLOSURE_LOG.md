<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 验证披露日志（DISCLOSURE_LOG）

> **性质**：记录一切对既定验证计划的偏离与历史状态修正（[MECHANISM.md](MECHANISM.md) M5）。
> **记录规范**：只记事实——什么变了、客观依据、影响、修正去向（M10）。
> **条目格式**：事实 / 依据 / 影响 / 修正去向。

## 首条（2026-09-12）：验证机制建立与历史状态补记

验证机制（MECHANISM.md M1–M11）与验证状态单一真相源（STATUS.md）于 2026-09-12 建立。建立时对公开仓验证文档与实际验证状态做了一次全局核对，以下 12 项为核对发现的不符/漂移及修正去向（一次性补记）。

### 漂移根因说明

本次核对同时确认：上述漂移并非故意隐瞒——EvoRule 迄今经历三次大的重构，大量功能（含部分形式化验证资产）在重构过程中移出至本仓之外的其他目录；漂移前的代码与文档大部分仍存续，其记录（包括 PASS、FAIL 乃至工具崩溃）均为如实记录。漂移的成因是重构后公开仓文档未同步更新、资产与文档的对应关系断裂，而非记录本身不诚实。本补记旨在重建对应关系、修正表述；对重构前历史记录的诚实性予以确认。

| # | 事实（核对发现） | 依据 | 影响 | 修正去向 |
| - | ---------------- | ---- | ---- | -------- |
| 1 | 三层漂移：代码与 CI 为 v0.5.0（2026-09-12，四条验证 CI 落地）；证据库冻结于 2026-08-18（全部产自 v0.3.1）；验证文档冻结于 2026-08-17 | 2026-09-12 全局核对 | 状态类声明整体与实况不符 | STATUS.md 建立（v0.5.0 快照）；不符文档按修正批次处理 |
| 2 | `evorule-tcb/verification/evidence/kani/` 下 16 个证据文件全部不符合 evidence/README.md 命名规范：`single.log` 含无名 FAILED 记录；`p4_t2.log` 为工具崩溃日志；`p4567_tmp.log`、`p8_11.log` 为 0KB 空文件；`ps_check.txt`、`ps_count.txt` 为进程诊断残留 | 对照 evidence/README.md §三 | TCB Kani 证据链整体失效 | 12 个 `git mv` 至 `_invalidated/`；4 个零价值文件 `git rm`（处置记录③） |
| 3 | 2026-09-09 enforce halt 语义修改了 proof 源码（`kani_proofs.rs`），对应证据未重跑 | git 提交历史 | 旧证据（已隔离）与当前代码不一致 | 阶段 2 于 v0.5.0 重跑归档 |
| 4 | plan v3 属性表 P0-4 标注「✅ 实跑†」与实测记录不符：两代方法均未 PASS，且该文档内部表述自相矛盾 | 2026-09-12 全局核对 | P0-4 状态与实况不符 | STATUS.md：P0-4 = ❌+🔵（proptest 兜底）；plan v3 状态列剥离（修正批次） |
| 5 | 原 INDEX.md 称 TLA+「⏳ 未实现」，实际 TLC 验证报告已存在（2026-07-25，降级模型 N_MAX=2） | `evorule-tcb/tla/TLC_VERIFICATION_REPORT.md` | TLA+ 侧覆盖被低估 | STATUS.md：P0-7/P0-8 = ❌+🟡 |
| 6 | GATE_REFERENCE.md L250 含与实况不符的完成声明（验证项未完成但标已完成） | 2026-09-12 全局核对 | 门禁文档与实况不符 | 修正批次：以五档词汇改写 |
| 7 | 根 README badge 数字与实况不符：「45 proofs (12 verified)」「Pending CI 33」与当前 37-proof（A 档 14 可跑 + B 档 23 不可运行）实况不符 | 对照 kani.yml 与 proof 清单 | 对外表述与实况不符 | 修正批次：badge 及正文（L10、L281-287、中文镜像 L770-776 等） |
| 8 | `evorule-reactor/docs/KANI.md` CI 段与 kani.yml 实况漂移；「10/11 PASS」为 2026-07-27 旧版本口径且未锚定版本 | 对照 kani.yml | reactor 文档与实况不符 | 修正批次（3 处） |
| 9 | 4 处拼写错误死引用（EVORULE_FORMAL_VERTICATION / VERTIFICATION 系列） | 全仓检索 | 引用失效 | 修正批次（保留 2 处登记性出现：DOCS_INDEX.md:159、check_doc_safety.py:383） |
| 10 | `evorule-tcb` 设计稿 P1–P21 proof 编号与 P0-xx/P1-xx 属性编号命名空间冲突 | 2026-09-12 全局核对 | 编号歧义 | MECHANISM.md M8 显式作废旧编号；映射表入 STATUS.md 附录 A |
| 11 | B 档 23 个 TCB proof 判定当前不可运行：实测 600s 全超时（其中 2 个实际跑 910s+）；3600s 仍超时（`verify_merge_safe` 跑满 3603s）；unwind 4/8 无改善 | kani.yml 头注实测记录（2026-09-11，Kani 0.67.0 / WSL Ubuntu 22.04） | P0-1/2/4/5/7/8 的 Kani 侧覆盖缺失 | STATUS.md 标 ❌；kani.yml B 档仅手动触发且允许失败 |
| 12 | 本仓 Coq 形式化为 0 行（部分形式化验证资产已随重构移出本仓，见漂移根因说明）；TLA+ 仅 1/6 模型（ExecuteTransition，N_MAX=2 降级） | 2026-09-12 全局核对 | L1 层覆盖声明须限定 | STATUS.md 如实标注 |

### 资产处置记录

| # | 事实 | 依据 | 影响 | 修正去向 |
| - | ---- | ---- | ---- | -------- |
| ① | `verification/INDEX.md` 删除 | 功能被 MECHANISM.md（规则）、STATUS.md（状态）、README.md（导航与登记）三方吸收，消灭第二状态源（M1） | 存量引用（DOCS_INDEX.md ×4、plan v3 ×2 等）需更新 | 修正批次更新全部引用 |
| ② | 根目录 `CHANGE_REQUEST_TEMPLATE.md` 删除 | 与 `.github/` 版内容漂移 5 行，双份维护实证 | 仅保留 `.github/` 版 | DOCS_INDEX.md 登记核对 |
| ③ | 4 个零证据价值文件（`ps_check.txt`、`ps_count.txt`、`p4567_tmp.log`、`p8_11.log`）直接 `git rm`，不随批隔离 | 进程诊断残留与 0KB 空文件，无历史证据价值（M3.3）；git 历史可溯 | 无 | — |

### 初值说明

- 本日志建立于 2026-09-12；上述 12 项核对发现与 3 项处置为机制建立时的一次性补记，此后所有偏离按时间顺序追加于下方。
- STATUS.md 初值快照为 v0.5.0（commit `5fac8bd`）；表中 ✅ 项的 v0.5.0 归档证据由整改阶段 2 重跑后落盘，届时同批更新证据列。
- 机制文档（MECHANISM.md / STATUS.md / DISCLOSURE_LOG.md / verification/README.md 重写稿）以未提交状态起草，定稿后由仓库维护者提交。
- 机制建立决策记录见 [docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md](../docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md)。

## 追加区

（此后按时间顺序追加，格式：日期 + 事实 / 依据 / 影响 / 修正去向）
