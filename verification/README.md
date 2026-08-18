<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 形式化验证文档系统

> **最后更新**：2026-08-17
> **版本对齐**：与 `Cargo.toml` 顶层 `version = "0.3.1"` 同步
> **入口**：本系统的一站式查询入口是 [INDEX.md](INDEX.md)

## 一、这是什么

EvoRule 的形式化验证工作会产生大量资产：**验证方案、运行脚本、证明源码、随机数据、运行证据（PASS/FAIL 日志）、验证报告**。它们过去分散在根目录、各 crate、脚本目录里，容易乱放、不保存、丢失、难以系统化查询。

本系统把这些资产**按约定归位、纳入 git、集中索引**，目标是：

1. **不丢失** —— 全部验证资产纳入 git 版本管理（含运行证据），历史可追溯；
2. **不乱放** —— 每个资产有唯一归属位置（见 §三 目录约定）；
3. **可查询** —— [INDEX.md](INDEX.md) 是唯一查询入口，按"七层验证方法论 + 各 crate + 资产类型"组织；
4. **不止 Kani** —— 覆盖白皮书 [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) 的完整七层验证体系（Coq / TLA+ / Kani / Verus / TLC / proptest / 差分测试 / 运行时验证 / 编译时门控）。

## 二、定位

本系统是验证资产的**组织与索引层**，不替代也不重复各 crate 的验证逻辑：

| 层         | 归属                                          | 说明                                             |
| ---------- | --------------------------------------------- | ------------------------------------------------ |
| 验证方案   | `verification/plan/`                          | 跨 crate 的指导性方案（白皮书）                  |
| 证明源码   | 各 crate `verification/`（如 `kani_proofs.rs`） | 证明/差分测试代码随 crate 走，crate 自治          |
| 运行脚本   | 各 crate 根 / `scripts/`                      | 单 crate 脚本随 crate；跨 crate 工具在 `scripts/` |
| 运行证据   | 各 crate `verification/evidence/` | 每次实跑的 PASS 日志 + 元数据                     |
| 查询入口   | `verification/INDEX.md`                       | 一站式索引，登记所有上述资产                     |

## 三、目录约定

```text
verification/
├── README.md               ← 本文件（系统说明）
├── INDEX.md                ← 验证资产总索引（唯一查询入口，必读）
├── plan/                   ← 验证方案与计划（指导性文档，随版本更新）
│   └── EVORULE_FORMAL_VERIFICATION_PLAN_v3.md  ← 白皮书（七层验证体系，当前有效）
├── evidence/               ← 验证证据（纳入 git，防丢失）
│   └── README.md           ← 证据规范（命名、元数据、收集方式）
└── scripts/                ← 跨 crate 验证工具（如证据收集器）
    └── collect-verification-evidence.ps1
```

## 四、使用方式

| 场景                             | 去这里                                             |
| -------------------------------- | -------------------------------------------------- |
| 查某个验证资产在哪 / 什么状态    | [INDEX.md](INDEX.md)                               |
| 查七层验证方法论 / 属性状态      | [plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) |
| 收集某次验证的 PASS 证据         | `scripts/collect-verification-evidence.ps1`（见 [evidence/README.md](evidence/README.md)） |
| 处理一次性散落日志                 | 直接丢弃（可重跑复现）；**禁止**迁入公开 `verification/`（会随仓发布） |
| 看某个 crate 的证明源码          | 该 crate `verification/` 目录                      |

## 五、维护规则（强制）

1. **新验证资产必登索引**：新增/迁移任何验证方案、脚本、证据、报告，必须同步登记到 [INDEX.md](INDEX.md)；
2. **证据必入库**：验证实跑的 PASS/FAIL 日志（含 commit / 工具链 / 时间戳元数据）必须保留在 `evidence/` 并纳入 git，禁止随手丢弃或放在 `.gitignore` 忽略区；
3. **方案版本对齐**：`plan/` 下方案文档的版本号必须与 `Cargo.toml` 顶层 `version` 一致；被新版取代的方案必须加 `[已废弃]` 横幅；
4. **一次性散落日志不入库**：根目录/临时位置的一次性运行日志（`cargo test` 原始输出等）**不迁入**公开 `verification/`（会随仓发布且可能含本机路径）；需要时直接重跑复现。只有收集器产出的**规范化证据**（按 [evidence/README.md](evidence/README.md) 命名、含元数据）才进 `evidence/` 入库；
5. **文档安全合规**：公开验证文档同样适用 `scripts/check_doc_safety.py` 的私有信息零泄露约束（不出现私有集合路径/文件名）。
