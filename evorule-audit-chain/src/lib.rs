// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `EvoRule` 审计链 no_std 内核（SC3-a 方案乙：SSOT 上提）
//!
//! 本 crate 是审计链算法的**单一真相源**，从 `evorule-reactor`（std 侧）上提：
//! - [`fact`]：Fact 数据形态（事实枚举 / FactId / IoType / TraceHit）——
//!   原子通信单元定义，随迁自 reactor/src/fact.rs（`std::sync::Arc` → `alloc::sync::Arc`，行为逐字一致）
//! - [`hash`]：稳定 JSON 序列化 + BLAKE3 链哈希——随迁自 reactor/src/hash.rs
//!   （序列化整体基于 `serde_json::Value/Map`，alloc 模式逐字节等价；tracing 观测调用
//!   不随迁——纯观测零行为影响，宿主侧日志由壳层按需补挂）
//!
//! # 设计原则
//! - `#![no_std]` + alloc：嵌入到 MCU（SC3）与 std 宿主（reactor/governance/cli
//!   经 re-export 消费）同一实现，消灭复刻漂移
//! - 确定性优先：同输入同输出，BTreeMap 固定键序
//! - 字节等价铁律：本上提不改变任何哈希输出（`hash_snapshot.txt` 快照 + 三方
//!   交叉验证测试随迁守护）
//!
//! # 消费方式
//! - `evorule-reactor`：`pub use evorule_audit_chain::fact / hash`（公开面零变化）
//! - `evorule-governance` / `evorule-cli`：既有 re-export 链路透传，无需改动
//! - `evorule-mcu`（SC3）：直接依赖本 crate 构建嵌入式最小内核

#![no_std]
extern crate alloc;

#[cfg(any(test, kani))]
extern crate std;

pub mod fact;
pub mod hash;
