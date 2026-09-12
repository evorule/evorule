<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 验证机制（MECHANISM）

> **版本对齐**：`Cargo.toml` workspace `version = "0.5.0"`（commit `5fac8bd`，2026-09-12）
> **性质**：验证机制的宪法性文档。M1–M11 约束所有验证文档的状态表述、证据效力、变更披露与信息分级。
> **配套文档**：[STATUS.md](STATUS.md)（验证状态唯一权威）｜[DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)（变更披露）｜[README.md](README.md)（导航与资产登记）
> **生效**：2026-09-12 首次成文。此前公开仓不存在成文验证机制，历史文档与本机制的差异一次性补记于 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 首条。

## 〇、范围与术语

本文约束的对象与术语：

| 术语       | 含义                                                                                       |
| ---------- | ------------------------------------------------------------------------------------------ |
| 验证资产   | 验证方案、证明源码（proof）、运行脚本、运行证据、验证报告的统称                             |
| 属性       | 被验证的安全/正确性命题，编号 P0-xx / P1-xx（目录见白皮书 plan v3 §属性体系）                |
| proof      | Kani 证明函数，以 Rust 函数名为唯一身份                                                     |
| 证据       | 实跑产生的 PASS/FAIL 日志 + 元数据（规范见 [evidence/README.md](evidence/README.md)）        |
| 验证文档   | `verification/` 及各 crate `verification/`、`docs/` 下表述验证状态的文档                     |

规则编号采用 **M 系**，与 `scripts/check_doc_safety.py` 的安全规则体系（R-门控1 / R3 / R-兄弟仓 / R-agent 身份零泄露等）相互独立、互为补充；M9 对后者为引用与扩展关系，不另起炉灶。

## M1 单一真相源（SSOT）

1. **[STATUS.md](STATUS.md) 是验证状态的唯一权威**。任何文档（根 README badge、GATE_REFERENCE.md、ROADMAP.md、各 crate KANI.md、白皮书 plan v3）需要表述验证状态时，必须引用 STATUS.md，不得独立断言状态、数量或结论。
2. 白皮书 plan v3 的属性状态列与追溯矩阵**让渡**权威地位，仅作方法论参考；其头部修订说明须指向 STATUS.md。
3. **CI workflow 的 yml 文件（含注释）是 CI 行为的唯一真相源**。文档对 CI 的描述与 yml 冲突时，以 yml 为准，文档须修正。
4. 原 INDEX.md 的状态登记功能由 STATUS.md（状态）与 README.md（资产登记）分别吸收，INDEX.md 已删除（见 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 资产处置记录①）。

## M2 五档状态词汇

替换旧三档词汇（✅实跑 / 🔧已实现未跑 / ⏳未实现）。STATUS.md 及一切引用处**只允许**以下五档（允许复合）：

| 档位        | 含义                        | 判定条件                                                       |
| ----------- | --------------------------- | -------------------------------------------------------------- |
| ✅ 当前实跑 | 当前版本、归档证据双齐      | ① proof 在当前版本可运行且 PASS；② 证据按 M3 规范归档          |
| 🟡 历史 PASS | 旧版本/旧方法下 PASS         | 须注明版本/方法与日期                                            |
| 🔵 间接覆盖 | proptest/差分/CI 测试级兜底 | 非形式化证明；须注明兜底手段                                    |
| ⏳ 计划中   | 已列入计划，未实现          | —                                                               |
| ❌ 不可运行 | proof 存在但实测不可运行    | 须注明实测依据（超时档位/日期/环境）                             |

- 复合表示：如 `❌+🔵`（proof 不可运行 + 间接覆盖兜底）、`❌+🟡`（proof 不可运行 + 旧方法历史 PASS）。
- 旧三档词汇自本机制生效后禁止新增使用；存量出现按 DISCLOSURE_LOG 记录的修正批次替换。

## M3 证据规范

1. **命名**：`<属性号>.<harness名>_<STATUS>_<commit短SHA>_<yyyyMMdd_HHmmss>.log`（及同名 `.stdout.txt`）。
   Label 扩展为「属性号.harness 名」的原因：同一属性常有多个 proof（如 P0-3 有 10 个 resolve_path 系列 proof），仅属性号无法区分。
2. **FAIL 证据强制字段**：harness 名 + 完整复现命令。二者缺一，不得入库。
3. **非证据**：工具崩溃日志、进程诊断残留（`ps` 输出等）、0KB 空文件不是证据，禁止入库。
4. **证据有效性判定**：证据文件名中的 commit SHA 须不早于 proof 源码最后一次变更的 commit；proof 代码变更后，旧证据自动失效，移入 `_invalidated/`。
5. **`_invalidated/` 隔离**：失效/被替代证据 `git mv` 至同级 `_invalidated/`（不物理删除，git 历史可溯），目录内置 README 说明作废原因与隔离日期。

## M4 发布同步

1. 验证文档头部的「版本对齐」声明必须与 `Cargo.toml` 当前 workspace `version` 一致；版本升级后全部验证文档同步更新。
2. release 检查清单挂接为可执行物：PR 模板检查项 + `release.yml` 步骤（核对 STATUS.md 快照版本 = 待发布版本、A 档 Kani CI 绿）。
3. 原 INDEX.md §七.3 的版本对齐规则并入本条，不再单独立规。

## M5 计划变更披露

1. [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 记录一切对既定验证计划的偏离：状态降级、方法变更、范围变更、时间表变更。
2. **ADR 触发情形**（写入 `docs/adr/`，其余进披露日志）：更换验证工具；修改符号输入建模策略；编号体系变更；状态档定义变更。
3. 机制建立前的历史状态修正以首条补记一次性记录（见 DISCLOSURE_LOG.md）。

## M6 对外表述规范

1. 对外声明（根 README、GATE_REFERENCE.md、发布说明等）**只能引用 ✅ 当前实跑** 的属性；🟡/🔵/⏳/❌ 不得包装为「已验证」。
2. 目标与现状分层表述：计划目标必须显式标注为计划，不得与现状混同。
3. 禁用未限定的比较级表述（如「超过 DO-178C Level A」）；任何对标须给出限定条件与依据。

## M7 历史诚实条款

1. 不删 git 历史，不重写已推送提交。
2. 修正 = 新文档 + 披露记录（DISCLOSURE_LOG.md），不是静默覆盖。
3. 资产迁移/隔离使用 `git mv`，保留 rename 追踪。
4. 对追溯历史者诚实陈述早期未成文阶段的实际过程，不掩饰。

## M8 编号统一

1. **P0-xx / P1-xx 属性编号是唯一强制属性编号**（定义于白皮书 plan v3 属性目录）。
2. proof 以 Rust 函数名为唯一身份，不使用独立 proof 编号。
3. **显式作废**：`evorule-tcb/verification/kani-formal-verification-design.md` 的 P1–P21 proof 编号体系（与属性编号命名空间冲突），映射关系收录于 [STATUS.md](STATUS.md) 附录 A；该设计稿转为历史文档。
4. 3 个无旧编号的 enforce proof（`verify_exec_enforce_*`）的属性归属（P0 域）见 STATUS.md 附录 A。

## M9 信息分级与内外隔离

1. 文档分两级：**公开级**（本仓承载）与**内部级**（存放于项目私有知识库）。公开仓文档只可陈述「内部级文档存在于私有知识库」这一事实，不得出现其路径、目录名、文件名或内容。
2. 本条是 `scripts/check_doc_safety.py` 既有安全规则（R-门控1 私有文档禁止入库、R3 私有路径零容忍、R-兄弟仓引用合规、R-agent 身份零泄露）在验证文档侧的**引用与扩展**，不替代。
3. 公开仓验证文档提交前必须通过 `check_doc_safety.py` 检查。

## M10 披露内容规范

1. 对外披露（含 DISCLOSURE_LOG.md 及一切公开仓文档）只记录**事实**：什么变了、客观依据（机制条款/实测数据）、影响（引用更新点）、修正去向。
2. 不记录：内部讨论过程、方案权衡细节、决策经过、评审/审批过程。
3. 内部级过程记录存放于私有知识库，不进入公开仓。

## M11 身份信息保护

1. 未经当事人同意，公开仓不披露任何个人或 LLM 的身份信息。
2. 唯一例外：Mr. Damu Zheng（项目指定对外信息发布者，锚点 [AUTHORS.md](../AUTHORS.md)）。
3. 对外署名信息统一以 `AUTHORS.md` 为准。

## 维护

- 本文档自身的变更属「状态档定义变更 / 编号体系变更」情形的，须经 ADR（M5）；
- 其余变更记入 DISCLOSURE_LOG.md；
- 版本对齐随每次 release 检查（M4）。
