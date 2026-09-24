<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 已失效证据隔离区（_invalidated）

> **性质**：本目录存放失效/被替代的验证证据（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.5）。
> **规则**：证据失效不物理删除，`git mv` 移入本目录，git 历史可溯原始位置与内容（`git log --follow`）；本 README 记录每批隔离的作废原因与隔离日期。

## 批次 1（2026-09-12）：TCB Kani 旧证据整体隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），12 个文件。

**共同失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3）：

1. **命名违规**：全部不符合证据命名规范（无属性号/harness 名/commit SHA/时间戳四要素，见[证据规范](../../../../../verification/evidence/README.md) §三）；
2. **版本脱节**：产出版本为 v0.3.1（证据库冻结于 2026-08-18），早于 proof 源码最后一次变更（2026-09-09，enforce halt 语义修改），按 M3.4 自动失效；
3. **部分不构成证据**：工具崩溃日志、未记 harness 名的 FAILED 输出不满足证据入库条件（M3.2/M3.3）。

**逐文件作废原因**（特有问题叠加于共同判定 1/2 之上）：

| 文件 | 特有问题 |
| ---- | -------- |
| p123_b_fill.log | — |
| p4_solo.log | — |
| p4_t1.log | — |
| p4_t2.log | 工具崩溃（Kani 运行中 panic: "No exit code?"，无验证结论） |
| p4_t3.log | — |
| p4_t4.log | — |
| p4567_v2.log | — |
| p4b_u4.log | — |
| p4cde.log | — |
| p5.log | — |
| p8d_min.log | — |
| single.log | VERIFICATION FAILED 未记 harness 名（M3.2 不满足） |

**替代证据**：v0.5.0 重跑 A 档 14 个 TCB proof 后按命名规范归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) 证据列）。

**同批其他处置**：另有 4 个零证据价值文件不适用隔离、直接删除（git 历史可溯）——`ps_check.txt`、`ps_count.txt`（进程诊断残留）与 `p4567_tmp.log`、`p8_11.log`（仅含单行 harness 标题、无验证结果的残文件）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 首条 #2、#3 及资产处置记录③。

## 批次 2（2026-09-13）：A 档 14 对证据随 Batch 1 载体回退失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `bdfb8d4`，产出于 2026-09-12）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：Batch 1 提交 `1c6ad84` 变更了被验证代码（`value.rs` 载体回退——CR-20260913-003 修订随 CR-20260913-004 生效；`executor.rs` mem::take 修复；`path.rs`/`domain.rs`/`determinism_proptest.rs` 测试面适配）与 proof 源码（`tests/kani/kani_proofs.rs`、`tests/kani/model.rs` 探针迁出与 harness 对齐），本批证据的 SHA 绑定早于 proof 源码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `1c6ad84` 重跑 A 档 14 个全 PASS（2026-09-13，WSL Kani 0.67.0），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-13 条目归档记录。

## 批次 3（2026-09-14）：A 档 14 对证据随 W3-1 proof 源码变更失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `1c6ad84`，产出于 2026-09-13）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：W3-1 提交 `1b340e5` 变更了 proof 源码（`tests/kani/kani_proofs.rs` 新增 7 个结构自检助手并为 23 个 B 档 harness 接线，CR-20260913-004 §3.8），本批证据的 SHA 绑定早于 proof 源码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `1b340e5` 重跑 A 档 14 个全 PASS（2026-09-14，WSL Kani 0.67.0，单 proof 0.3~4.0s），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-14 条目。

## 批次 4（2026-09-14）：A 档 14 对证据随 W3-3 proof 源码变更失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `1b340e5`，产出于 2026-09-14）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：W3-3 提交 `90b77aa` 变更了 proof 源码（`tests/kani/model.rs` obj() 由引用对+深克隆改为 `object_from_pairs_owned`，`tests/kani/kani_proofs.rs` 23 个 B 档 harness 构造调用点适配，CR-20260913-004 §3.10），本批证据的 SHA 绑定早于 proof 源码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `90b77aa` 重跑 A 档 14 个全 PASS（2026-09-14，WSL Kani 0.67.0，单 proof 0.3~4.0s），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-14 W3-3 条目。

## 批次 5（2026-09-14）：A 档 14 对证据随 W3-4 unwind 校准失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `90b77aa`，产出于 2026-09-14）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：W3-4 提交 `627330a` 变更了 proof 源码（`tests/kani/kani_proofs.rs` 4 处 unwind 属性按 W3-2 校准表精确化（instruction 32 / all 16 / deterministic 补 16 / domain_depth 16）+ 清理 W3-3 临时 canary，CR-20260913-004 §3.11），本批证据的 SHA 绑定早于 proof 源码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `627330a` 重跑 A 档 14 个全 PASS（2026-09-14，WSL Kani 0.67.0，单 proof 0.3~4.0s），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-14 W3-4 条目。

## 批次 6（2026-09-14）：A 档 14 对证据随规则清理 proof 源码变更失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `627330a`，产出于 2026-09-14）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：规则清理提交 `10c743d` 变更了被验证代码与 proof 源码（CR-20260914-001：collect/merge 元指令退役——`exec_collect`/`exec_merge`/`substitute_template` 删除、`META_INSTRUCTION_TYPES` 收窄 5 种；`tests/kani/kani_proofs.rs` P15/P16/P17 删除（37→34）、`tests/kani/model.rs` `any_instruction` %6→%4），本批证据的 SHA 绑定早于 proof 源码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `25c0cc0`（与 `10c743d` proof 源码一致，仅差 test.js）重跑 A 档 14 个全 PASS（2026-09-14，WSL Kani 0.67.0，单 proof 秒级；18/18 = TCB 14 + reactor 4），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**同批跨仓处置**：`evorule-reactor/verification/evidence/kani/` 4 对旧 PASS 证据（P0-11/P1-5 锚定 `03643aa`、P1-3/P1-6 锚定 `bdfb8d4`，proof 源码未变更但被验证依赖 TCB 生产代码变更，谨慎起见复跑替代）隔离至该仓同级 `_invalidated/`；P0-11 修复前 3 份 FAIL 过程证据按 STATUS 原注记原地保留。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-14 规则清理条目。

## 批次 7（2026-09-15）：A 档 14 对证据随 lint 清零生产源码变更失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `25c0cc0`，产出于 2026-09-14）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：提交 `35b6182` 变更了 TCB 生产源码（`src/value.rs` `try_insert` 增 `#[allow(clippy::map_entry)]` 与论证注释、`src/executor.rs` `META_INSTRUCTION_TYPES` 数组 rustfmt 折行与 EOF 换行——均零语义，监督核查 R02/R03 diff 级核验），本批证据的 SHA 绑定早于被验证代码最后一次变更，按 M3.4 自动失效。

**替代证据**：于 `34c841d` 重跑 A 档 14 个全 PASS（2026-09-15，WSL Kani 0.67.0，单 proof 0.3~4.0s；合计 18/18 = TCB 14 + reactor 4），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**同批跨仓处置**：`evorule-reactor/verification/evidence/kani/` 4 对旧 PASS 证据（锚定 `25c0cc0`）隔离至该仓同级 `_invalidated/` 批次 2（reactor `src/pure.rs` 同批 rustfmt import 排序，零语义）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-15 lint 清零条目。

## 批次 8（2026-09-18）：A 档 14 对证据随仓库提交历史整理失效隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），28 个文件（14 对 `.log` + `.stdout.txt`，命名锚定 `34c841d`，产出于 2026-09-15）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：2026-09-18 仓库提交历史整理（公开历史 commit message 规范化清洗，全链 commit 重写、版本 tag 重打），原基线 `34c841d` 在整理后历史中不复存在，本批证据的 SHA 绑定失效；被验证代码与 proof 源码零变更（新基线 `a3d728f` 树与 `34c841d` 树一致），按 M3.4 以复跑替代。

**替代证据**：于 `a3d728f` 重跑 A 档 14 个全 PASS（2026-09-18，WSL Kani 0.67.0，单 proof 0.3~4.0s；合计 18/18 = TCB 14 + reactor 4），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-3/P0-6 证据列）。

**同批跨仓处置**：`evorule-reactor/verification/evidence/kani/` 4 对旧 PASS 证据（锚定 `34c841d`）隔离至该仓同级 `_invalidated/` 批次 3（reactor `src/invariants.rs` 单元测试辅助 `set_io_result` 同批适配 kani cfg Object 后端 API——`entry` → `contains_key` + `get_mut`/`insert`，proof 函数与生产代码零变更）。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-18 条目。

## 批次 9（2026-09-24）：15 个 `.FAIL.raw` 环境故障残留隔离

**来源**：`../`（`evorule-tcb/verification/evidence/kani/`），15 个 `.raw` 文件（2026-09-24 本地 Kani 批验尝试的原始输出，未入库）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3）：

1. **命名违规**：`.raw` 为脚本直出文件名，不符合 M3.1 四要素命名规范（无 commit SHA/时间戳）；
2. **不构成证据**：内容为 WSL rustup 工具链安装故障原始输出（`os error 39: Directory not empty`，组件重命名冲突）——**验证未实际执行**，无验证结论（M3.2 不满足）；
3. **涉及 A 档 proofs（P0-3 系 12 个、P0-6 系 3 个）均有现行有效替代证据**（批次 8 于 `a3d728f` 复跑 18/18 PASS，见 [STATUS.md](../../../../../verification/STATUS.md)）；
4. `P0-5.verify_exec_enforce_permutation_equivalence.FAIL.raw`：排列等价新证明首次尝试因同一环境故障未跑成，无有效证据；该证明按 B 档登记（[STATUS.md](../../../../../verification/STATUS.md) 附录 B），待环境修复后人工验证。

**处置**：移入本隔离区（未入库文件，直接 `Move-Item`，无 git 历史锚定）；故障根因（rustup 工具链目录残留冲突）留待验证环境专项处理。

**披露记录**：本次为提交前置门禁（check_status_sync S4）整改，随 evorule-discipline crate 抽取批次提交。
