// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EvoRule MCU no_std 内核——紧凑规则包 + RAM WAL + tick 时钟 + 同步执行器
//!
//! # 定位
//! 嵌入式设备（Cortex-M / RISC-V）上的确定性规则引擎最小内核：
//! - `no_std` + `alloc`，零 std 依赖
//! - 审计链哈希经 [`evorule_audit_chain`] 单一真相源——与 std 宿主
//!   （evorule-reactor）对同一事实流产出**逐字节一致**的 WAL 行与链尾哈希
//! - 执行器事实发射语义与 reactor 主循环逐行同构（Command / StateTransition /
//!   TransitionTrace / Stable / Error / Violation / IoRequest）
//!
//! # 模块
//! - [`pack`]：紧凑规则包（postcard 编码的 core_eval 规则数组）加载
//! - [`wal`]：WAL 后端接口（[`McuWalStore`]）与 RAM 后端（`FactWalStore`
//!   同构语义：append-only、哈希字段原样保存、write-ahead 先于状态更新）
//! - [`wal_flash`]：Flash WAL 纯逻辑层（介质抽象 + 环形页布局 + 断电
//!   截断恢复；真板介质随实机批实现同一 [`wal_flash::FlashMedium`] trait）
//! - [`clock`]：tick 时钟（确定性单调计数，替代 `Instant`）
//! - [`exec`]：同步执行器 facade（事件 → `execute_transition` → 审计 → WAL）
//! - [`alloc_tlsf`]（feature `tlsf-alloc`）：TLSF 全局分配器包装——板级
//!   二进制的 `#[global_allocator]` 供给
//!
//! # 已知边界（无硬件批）
//! `io_request` 元指令 fail-visible：IoRequest 事实入链留痕后返回
//! [`McuError::IoUnsupported`]；I/O 恢复执行（`__io_results__` 注入 + 指令
//! 重推）随实机批另行立项。
#![no_std]
extern crate alloc;
#[cfg(any(test, kani))]
extern crate std;

#[cfg(feature = "tlsf-alloc")]
pub mod alloc_tlsf;
pub mod clock;
pub mod error;
pub mod exec;
pub mod pack;
pub mod wal;
pub mod wal_flash;

pub use clock::TickClock;
pub use error::McuError;
pub use exec::{McuExecutor, RunReport, DEFAULT_MAX_QUEUE_LEN, DEFAULT_MAX_STEPS_PER_RUN};
pub use pack::RulePack;
pub use wal::{McuWalRecord, McuWalStore, RamWalStore};
