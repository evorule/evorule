<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 已失效证据隔离区（_invalidated）

> **性质**：本目录存放失效/被替代的验证证据（[MECHANISM.md](../../../../verification/MECHANISM.md) M3.5）。
> **规则**：证据失效不物理删除，`git mv` 移入本目录，git 历史可溯原始位置与内容（`git log --follow`）；本 README 记录每批隔离的作废原因与隔离日期。

## 批次 1（2026-09-12）：P0-9/P0-10 差分旧证据被替代隔离

| 文件 | 作废原因 |
| ---- | -------- |
| P0-9-P0-10_PASS_8b2932e_20260814_111450.log | 被替代：产出版本 8b2932e（2026-08-14 归档，Windows 平台、PROPTEST_CASES 默认值），已被 v0.5.0 重跑证据替代（M3.5） |
| P0-9-P0-10_PASS_8b2932e_20260814_111450.stdout.txt | 同上（同一运行的完整输出） |

**替代证据**：`../P0-9-P0-10_PASS_bdfb8d4_20260912_173435.log` / `.stdout.txt`（v0.5.0，WSL，PROPTEST_CASES=1000）。

**同批说明**：`../` 另保留 2026-09-12 两次失败重试记录（`*_FAIL_bdfb8d4_20260912_145816*`：编译期 ENOMEM；`*_FAIL_bdfb8d4_20260912_161421*`：低并行下编译超时），系如实记录的过程证据，非失效隔离对象。

**披露记录**：见 [DISCLOSURE_LOG.md](../../../../verification/DISCLOSURE_LOG.md) 阶段 2 执行条目。
