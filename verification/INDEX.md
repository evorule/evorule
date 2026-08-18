<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 形式化验证资产总索引

> **最后更新**：2026-08-17
> **版本对齐**：与 `Cargo.toml` 顶层 `version = "0.3.1"` 同步
> **性质**：L1 公开层验证资产的一站式查询入口。验证方法论见 [plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)（七层验证体系）。
>
> **维护规则**：新增/迁移任何验证方案、脚本、证据、报告，必须在本索引登记（见 §七）。

---

## 一、资产总览

| 资产类型     | 归属位置                                        | 说明                                     |
| ------------ | ----------------------------------------------- | ---------------------------------------- |
| 验证方案     | `verification/plan/`                            | 指导性方案（白皮书 + 各 crate 验证设计） |
| 证明源码     | 各 crate `verification/` + `tests/`             | Kani proof / 差分测试 / proptest 源码    |
| 运行脚本     | `scripts/` + 各 crate 根                        | 证据收集、Kani 运行、产物收集            |
| 运行证据     | 各 crate `verification/evidence/` | 实跑 PASS/FAIL 日志 + 元数据（纳入 git） |
| 验证报告     | 各 crate 根（如 `DETERMINISM_REPORT.md`）       | 分析/结论性报告                          |

---

## 二、验证方案与计划（`verification/plan/`）

| 文档                                                              | 范围     | 状态            | 说明                                                       |
| :---------------------------------------------------------------- | :------- | :-------------- | :--------------------------------------------------------- |
| [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) | 全机制层 | ✅ 指导性现行版 | 七层验证体系、P0/P1/P2 属性目录、追溯矩阵、分阶段路线图    |
| [evorule-tcb 的 Kani 验证设计](https://github.com/evorule/evorule/blob/main/evorule-tcb/verification/kani-formal-verification-design.md) | evorule-tcb | ✅ 设计稿 | P1-P21 证明清单、结构化符号输入、运行命令（详见 §四.1） |

> 白皮书 v3 是七层体系的"宪法"；各 crate 的专项验证设计是"实施细节"。两者保持追溯关系：属性编号（P0-xx）在两边一致。

---

## 三、七层验证资产分布

按白皮书 §三 的七层方法论组织。各层列出：**证明源码 / 脚本 / 证据 / 关联属性**。

### L1 数学形式化（Coq + TLA+ + TLAPS）

| 资产               | 位置                       | 状态     | 关联属性     |
| ------------------ | -------------------------- | -------- | ------------ |
| TLA+ 模型 + TLC 报告 | `evorule-tcb/tla/`（规划） | ⏳ 未实现 | P0-5/7/8     |
| Coq 形式化（JsonValue.v 等） | `evorule-tcb/coq/`（规划） | ⏳ 未实现 | P0-3~P0-8    |
| TLAPS 数学归纳     | 阶段 3                     | ⏳ 未实现 | P0 全部      |

### L2 代码级演绎验证（Kani + Verus）

| 资产                              | 位置                                                              | 状态     | 关联属性          |
| --------------------------------- | ----------------------------------------------------------------- | -------- | ----------------- |
| TCB Kani 证明（P1-P21 设计）      | [evorule-tcb/verification/kani-formal-verification-design.md](https://github.com/evorule/evorule/blob/main/evorule-tcb/verification/kani-formal-verification-design.md) | 🔧 设计稿，待实跑 | P0-1~P0-8 |
| Reactor Kani 证明（11 proof）     | [evorule-reactor/verification/kani_proofs.rs](../evorule-reactor/verification/kani_proofs.rs) | ✅ 10 PASS + 1 TIMEOUT（2026-08-05） | P0-11 / P1-1~P1-6 |
| Kani 运行脚本（reactor）          | `evorule-reactor/run_kani_proofs.sh`                               | ✅       | —                 |
| Kani 产物收集脚本（reactor）      | `evorule-reactor/collect_kani_artifacts.sh`                        | ✅       | —                 |
| Kani 运行脚本（cli）              | `evorule-cli/run_kani.sh`                                          | ✅       | —                 |
| Verus 规约                        | —                                                                 | ⏳ 未实现 | P0-5/12           |

### L3 模型检测（TLA+ TLC + Kani bounded）

| 资产                     | 位置                        | 状态     | 关联属性 |
| ------------------------ | --------------------------- | -------- | -------- |
| ExecuteTransition.tla    | `evorule-tcb/tla/`（规划）  | ⏳ 未实现 | P0-5     |
| ReactorStateMachine.tla  | `evorule-reactor/tla/`（规划） | ⏳ 未实现 | P0-11    |

### L4 属性测试（proptest + cargo-fuzz）

| 资产                                     | 位置                                                             | 状态     | 关联属性          |
| ---------------------------------------- | ---------------------------------------------------------------- | -------- | ----------------- |
| TCB 确定性属性测试（19 用例）            | [evorule-tcb/tests/determinism_proptest.rs](../evorule-tcb/tests/determinism_proptest.rs) | ✅ 实跑   | P0-3/4/5/6        |
| TCB 集成测试                             | [evorule-tcb/tests/integration_test.rs](../evorule-tcb/tests/integration_test.rs) | ✅ 实跑   | P1-8/9            |
| Reactor 集成测试                         | [evorule-reactor/tests/integration_test.rs](../evorule-reactor/tests/integration_test.rs) | ✅ 实跑   | P1-3~P1-6         |
| cargo-fuzz 模糊测试                      | —                                                                | ⏳ 阶段 3 | P0-3/4/5          |

### L5 差分测试（differential testing）

| 资产                                    | 位置                                                             | 状态   | 关联属性       |
| --------------------------------------- | ---------------------------------------------------------------- | ------ | -------------- |
| Reactor 差分测试（pure vs reactor 等）  | [evorule-reactor/verification/differential_test.rs](../evorule-reactor/verification/differential_test.rs) | ✅ 实跑 | P0-12          |
| Governance 差分测试（rewind/审计链）    | [evorule-governance/verification/differential_test.rs](../evorule-governance/verification/differential_test.rs) | ✅ 实跑 | P0-9/10/14/15  |
| Governance 审计链端到端                 | [evorule-governance/tests/end_to_end_audit_chain.rs](../evorule-governance/tests/end_to_end_audit_chain.rs) | ✅ 实跑 | P0-14          |

### L6 运行时验证（invariants + hash chain）

| 资产                  | 位置                                                        | 状态   | 关联属性 |
| --------------------- | ----------------------------------------------------------- | ------ | -------- |
| Reactor 不变式自检    | [evorule-reactor/src/invariants.rs](../evorule-reactor/src/invariants.rs) | ✅ 实跑 | P1-1~P1-6 |
| Governance 审计链     | [evorule-governance/src/auditor.rs](../evorule-governance/src/auditor.rs) | ✅ 实跑 | P0-14/15 |

### L7 编译时门控（build.rs + clippy）

| 资产                          | 位置                                                      | 状态   | 关联属性 |
| ----------------------------- | --------------------------------------------------------- | ------ | -------- |
| 各 crate build.rs 门禁        | `evorule-tcb/build.rs` / `evorule-reactor/build.rs` / `evorule-governance/build.rs` / `evorule-cli/build.rs` | ✅ 实跑 | G8 / T4-T14 / T15 / T16 |
| 门禁参考                      | [GATE_REFERENCE.md](../GATE_REFERENCE.md)                 | ✅ 实跑 | —        |

---

## 四、各 crate 验证资产

### 4.1 evorule-tcb（TCB 基础层）

| 资产                                   | 位置                                                                 | 状态       |
| -------------------------------------- | -------------------------------------------------------------------- | ---------- |
| Kani 验证设计（P1-P21）                | [verification/kani-formal-verification-design.md](https://github.com/evorule/evorule/blob/main/evorule-tcb/verification/kani-formal-verification-design.md) | 🔧 设计稿   |
| Kani proof 源码（规划）                | `tests/kani.rs` + `tests/kani/`（按设计稿 §六）                       | ⏳ 待实施   |
| 确定性属性测试（proptest）             | [tests/determinism_proptest.rs](../evorule-tcb/tests/determinism_proptest.rs) | ✅ 实跑     |
| 集成测试                              | [tests/integration_test.rs](../evorule-tcb/tests/integration_test.rs) | ✅ 实跑     |
| 确定性报告                            | [DETERMINISM_REPORT.md](../evorule-tcb/DETERMINISM_REPORT.md)         | ✅ 现行     |

### 4.2 evorule-reactor（反应器层）

| 资产                       | 位置                                                                 | 状态       |
| -------------------------- | -------------------------------------------------------------------- | ---------- |
| Kani 证明（11 proof）      | [verification/kani_proofs.rs](../evorule-reactor/verification/kani_proofs.rs) | ✅ 10 PASS + 1 TIMEOUT |
| Kani 验证指南               | [docs/KANI.md](../evorule-reactor/docs/KANI.md)                           | ✅ 现行     |
| 差分测试                   | [verification/differential_test.rs](../evorule-reactor/verification/differential_test.rs) | ✅ 实跑     |
| 集成测试                   | [tests/integration_test.rs](../evorule-reactor/tests/integration_test.rs) | ✅ 实跑     |
| 复杂规则测试               | [tests/complex_rule_test.rs](../evorule-reactor/tests/complex_rule_test.rs) | ✅ 实跑     |
| Kani 运行脚本              | `run_kani_proofs.sh` / `collect_kani_artifacts.sh`                    | ✅          |

### 4.3 evorule-governance（治理层机制）

| 资产                     | 位置                                                                   | 状态   |
| ------------------------ | ---------------------------------------------------------------------- | ------ |
| 差分测试                 | [verification/differential_test.rs](../evorule-governance/verification/differential_test.rs) | ✅ 实跑 |
| 审计链端到端测试         | [tests/end_to_end_audit_chain.rs](../evorule-governance/tests/end_to_end_audit_chain.rs) | ✅ 实跑 |
| 会话隔离测试             | [tests/session_isolation_test.rs](../evorule-governance/tests/session_isolation_test.rs) | ✅ 实跑 |

### 4.4 evorule-cli（CLI 工具）

| 资产                     | 位置                                                     | 状态   |
| ------------------------ | -------------------------------------------------------- | ------ |
| CLI Rust 集成测试        | [tests/cli_test.rs](../evorule-cli/tests/cli_test.rs)    | ✅ 实跑 |
| 端到端测试（TAP）        | [tests/e2e.sh](../evorule-cli/tests/e2e.sh)              | ✅ 实跑 |
| Kani 运行脚本            | `run_kani.sh`                                            | ✅      |

---

## 五、验证证据（`verification/evidence/`）

证据 = **实跑 PASS/FAIL 日志 + 元数据**（commit SHA / 工具链版本 / 时间戳 / 平台 / 运行命令）。规范见 [evidence/README.md](evidence/README.md)。

| 位置                                        | 内容                                     |
| ------------------------------------------- | ---------------------------------------- |
| `evorule-reactor/verification/evidence/`    | Reactor 差分测试实跑证据（收集器产出）   |
| `evorule-governance/verification/evidence/` | Governance 差分测试实跑证据（收集器产出） |

---

## 六、属性 → 证据 追溯

每个 P0/P1 属性的验证覆盖与证据链接，以白皮书 [§五 追溯矩阵](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md#五追溯矩阵) 为单一真相源；本索引 §三/§四 给出具体资产位置。新属性/新证据上线时，两边必须同步更新。

---

## 七、维护规则（强制）

1. **新资产必登本索引**：新增/迁移验证方案、脚本、证据、报告时，必须在本索引登记，并在 [README.md](README.md) §五 确认符合约定；
2. **证据必入库**：实跑证据纳入 git（`.gitignore` 不得忽略 `verification/evidence/`），命名遵循 [evidence/README.md](evidence/README.md)；
3. **方案版本对齐**：方案文档版本号与 `Cargo.toml` 顶层 `version` 一致；被取代的方案加 `[已废弃]` 横幅；
4. **追溯一致性**：属性编号（P0-xx）在白皮书、本索引、各 crate 设计稿、证据文件中必须一致；
5. **文档安全合规**：公开验证文档适用 `scripts/check_doc_safety.py` 的私有信息零泄露约束。
