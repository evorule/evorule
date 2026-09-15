<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 已失效证据隔离区（_invalidated）

> **性质**：本目录存放失效/被替代的验证证据（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.5）。
> **规则**：证据失效不物理删除，`git mv` 移入本目录，git 历史可溯原始位置与内容（`git log --follow`）；本 README 记录每批隔离的作废原因与隔离日期。

## 批次 1（2026-09-14）：reactor 旧 4 对证据随 69 号清理失效隔离

**来源**：`../`（`evorule-reactor/verification/evidence/kani/`），8 个文件（4 对 `.log` + `.stdout.txt`，命名锚定 `03643aa` / `bdfb8d4`，产出于 2026-09-12）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：69 号清理（CR-20260914-001，提交 `10c743d` + `25c0cc0`）变更了被验证依赖的 TCB 生产代码（collect/merge 元指令退役，v0.6.0 破坏性变更）。reactor 4 个 CI proof 的源码本身未变更，但证据 SHA 锚定的被验证代码环境已失效，谨慎起见复跑替代并隔离旧证据。

**逐文件清单**（适用共同判定，无逐文件特有问题）：

| 文件对 | 归档时结果 |
| ------ | ---------- |
| P0-11.invariant_cause_queue_sync_PASS_03643aa_20260912_225316 | PASS（1.60s，647 断言 0 失败，CI 同参数） |
| P1-3.invariant_version_monotonic_PASS_bdfb8d4_20260912_145126 | PASS |
| P1-5.command_does_not_decrease_queue_PASS_03643aa_20260912_225328 | PASS（`--default-unwind 4`，0.86s） |
| P1-6.max_rounds_termination_PASS_bdfb8d4_20260912_145136 | PASS |

**替代证据**：于 `25c0cc0` 复跑 4 个 CI proof 全 PASS（2026-09-14，WSL Kani 0.67.0），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-11/P1-3/P1-5/P1-6 证据列）。

**说明**：P0-11 修复前 3 份超时 FAIL 过程证据（`FAIL_bdfb8d4_20260912_*`）按 M3.2 合规存档，保留于 `../` 主目录，不属于本隔离区范围。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-14「69 号清理」条目（隔离执行）与 2026-09-15「REM-1 门禁整改」条目（本 README 补建）。

## 批次 2（2026-09-15）：reactor 4 对证据随 lint 清零批次失效隔离

**来源**：`../`（`evorule-reactor/verification/evidence/kani/`），8 个文件（4 对 `.log` + `.stdout.txt`，命名锚定 `25c0cc0`，产出于 2026-09-14）。

**失效判定**（[MECHANISM.md](../../../../../verification/MECHANISM.md) M3.4）：主仓提交 `35b6182` 变更了被验证依赖（reactor `src/pure.rs` 测试模块 import 排序——零语义，监督核查 R03 diff 级核验；TCB `src/value.rs`/`src/executor.rs` rustfmt 与 lint 豁免标注——零语义）。按 M3.4「证据 SHA 绑定早于被验证代码或依赖最后一次变更即失效」，随主仓同批 A 档证据一并重置。

**逐文件清单**（适用共同判定，无逐文件特有问题）：

| 文件对 | 归档时结果 |
| ------ | ---------- |
| P0-11.invariant_cause_queue_sync_PASS_25c0cc0_20260914_190211 | PASS |
| P1-3.invariant_version_monotonic_PASS_25c0cc0_20260914_190211 | PASS |
| P1-5.command_does_not_decrease_queue_PASS_25c0cc0_20260914_190211 | PASS（`--default-unwind 4`） |
| P1-6.max_rounds_termination_PASS_25c0cc0_20260914_190211 | PASS（`--default-unwind 4`） |

**替代证据**：于 `34c841d` 重跑 4 个 CI proof 全 PASS（2026-09-15，WSL Kani 0.67.0；合计 18/18 = TCB 14 + reactor 4），按 M3.1 命名归档于 `../`（见 [STATUS.md](../../../../../verification/STATUS.md) P0-11/P1-3/P1-5/P1-6 证据列）。

**说明**：P0-11 修复前 3 份超时 FAIL 过程证据（`FAIL_bdfb8d4_20260912_*`）按 M3.2 合规存档，保留于 `../` 主目录，不属于本隔离区范围。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../../verification/DISCLOSURE_LOG.md) 2026-09-15 lint 清零条目。
