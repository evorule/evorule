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

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]

mod channel;
mod error;
mod fact;
mod facts_log;
mod reactor;
mod stable_detector;
mod state;
mod wal;

pub use channel::{ChannelPair, EventReceiver, EventSender, FactReceiver, FactSender};
pub use error::ReactorError;
pub use fact::{Fact, FactId, FactIdGenerator, IoType};
pub use facts_log::{FactsLog, FactsLogError};
pub use reactor::{Reactor, ReactorBuilder, ReactorHandle};
pub use stable_detector::StableDetector;
pub use wal::{
    fact_from_json, fact_to_json, read_wal, serde_to_tcb, tcb_to_serde, WalError, WalWriter,
};
