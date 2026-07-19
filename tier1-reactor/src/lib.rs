// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TheEquation 反应式执行器
//!
//! # 设计原则
//! - 事实驱动：所有交互通过 Fact 通道进行
//! - 单一串行通道：保证调度确定性
//! - 稳定检测：队列空 + 无待处理 I/O = 稳定
//! - 无状态泄漏：所有状态由反应器维护
//! - Append-Only 审计链：所有 Fact 追加到 FactsLog，支持审计重放
//!
//! # 架构
//! ```text
//! 用户/治理层 → FactSender → [command mpsc] → 反应器
//!                                                ↓
//!                                          调用 TCB 核心
//!                                                ↓
//!                                  产生新 Fact → event broadcast → 用户/I/O 订阅者/审计器
//!                                                ↓
//!                                  所有 Fact → FactsLog（审计链）
//! ```
//!
//! # 模块结构
//! - `fact` — Fact 枚举、FactId、IoType
//! - `channel` — 双通道封装（command + event）
//! - `reactor` — 反应器核心引擎
//! - `state` — 反应器内部状态
//! - `facts_log` — Append-Only 审计链
//! - `stable_detector` — 稳定检测逻辑
//! - `io_timeout_policy` — I/O 超时阈值策略（阶段3-1.4）
//! - `metrics` — 可观测性指标（阶段3-1.5）
//! - `time_machine` — 时间机器 rewind/fork/diff/replay（阶段5，软回滚模式）
//! - `debug_control` — 调试器级控制 pause/resume/step（阶段6，第四组）
//! - `pure` — 纯逻辑模块，Kani 形式化验证准备（阶段7，第五组）
//! - `semantic_invariants` — 声明式语义不变式引擎（JSON 驱动，复用 tier0 域原语）
//!
//! # 使用示例
//! ```ignore
//! use tier1_reactor::{Reactor, Fact, FactId};
//! use tier0_tcb::JsonValue;
//!
//! let core_eval = vec![];  // 从 core_eval.json 加载
//! let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
//! let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();
//!
//! // 提交命令
//! tx.send(Fact::Command {
//!     id: FactId(1),
//!     instruction: JsonValue::object_from_pairs(&[
//!         ("type", JsonValue::string("increment")),
//!         ("params", JsonValue::object_from_pairs(&[
//!             ("attr", JsonValue::string("x")),
//!             ("delta", JsonValue::Integer(5)),
//!         ])),
//!     ]),
//! }).unwrap();
//!
//! // 等待 Stable 事实（broadcast recv 返回 Result）
//! while let Ok(fact) = rx.recv().await {
//!     if let Fact::Stable { final_snapshot, .. } = fact {
//!         println!("完成: {:?}", final_snapshot);
//!         break;
//!     }
//! }
//! ```

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]

mod channel;
mod debug_control;
mod error;
mod fact;
mod facts_log;
#[cfg(feature = "ffi")]
mod ffi;
mod invariants;
mod io_timeout_policy;
mod metrics;
mod phase;
#[allow(dead_code)]
mod pure;
mod reactor;
mod rule_safety;
mod rule_validator;
mod semantic_invariants;
mod stable_detector;
mod state;
mod time_machine;
mod wal;

pub use channel::{ChannelPair, EventReceiver, EventSender, FactReceiver, FactSender};
pub use debug_control::{BreakCondition, DebugControl};
pub use error::ReactorError;
pub use fact::{ControlFlowType, Fact, FactId, FactIdGenerator, IoType};
pub use facts_log::{FactsLog, FactsLogError};
pub use invariants::InvariantViolation;
pub use io_timeout_policy::{IoTimeoutPolicy, TimeoutThreshold};
pub use metrics::ReactorMetrics;
pub use phase::{PhaseContext, ReactorPhase};
pub use reactor::{PendingIoEntry, Reactor, ReactorBuilder, ReactorHandle, ReactorStateSnapshot};
pub use rule_safety::{RuleSafetyAnalyzer, SafetyMetrics, SafetyReport};
pub use rule_validator::{RuleValidator, ValidationError, ValidationResult};
pub use semantic_invariants::{
    SemanticInvariantError, SemanticInvariantRule, SemanticInvariantViolation, Severity,
};
pub use stable_detector::StableDetector;
pub use time_machine::{diff, fork, replay, rewind, PayloadDiff, RewindSnapshot};
pub use wal::{
    fact_from_json, fact_to_json, read_wal, serde_to_tcb, tcb_to_serde, WalError, WalWriter,
};
