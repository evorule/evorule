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
