//! TheEquation TCB Core - 纯计算内核
//!
//! # 设计原则
//! - 零依赖（`#![no_std]` 兼容）
//! - 纯函数（所有函数无副作用）
//! - 确定性强（`BTreeMap` + 确定性迭代）
//! - 永不 panic（路径解析返回 `Option`）
//!
//! # 公开接口
//! - `execute_transition()`：执行一步状态转换
//! - `JsonValue`：JSON 数据模型
//! - `TcbError`：错误类型

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
// 永不 panic 保障：禁止以下 panic-prone 模式
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

extern crate alloc;

// 模块声明（按依赖顺序：基础类型在前）
pub mod domain;
pub mod error;
pub mod executor;
pub mod path;
pub mod transition;
pub mod value;

// Kani 形式化验证（仅在 kani feature 启用时编译）
#[cfg(kani)]
mod proofs;

// 核心类型重导出（仅公开稳定的 API）
pub use error::TcbError;
pub use transition::{execute_transition, TransitionResult};
pub use value::JsonValue;
