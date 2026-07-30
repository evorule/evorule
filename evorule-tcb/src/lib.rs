// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `EvoRule` TCB Core - 纯计算内核
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
//!
//! # 快速入门
//!
//! ```
//! use evorule_tcb::JsonValue;
//! use std::collections::BTreeMap;
//!
//! // 构造一个 JsonValue 对象
//! let mut map = BTreeMap::new();
//! map.insert("name".to_string(), JsonValue::string("Alice"));
//! map.insert("age".to_string(), JsonValue::Integer(30));
//! let value = JsonValue::object(map);
//!
//! // 类型检查与取值
//! assert!(value.is_object());
//! let age = value.get("age").and_then(|v| v.as_i64());
//! assert_eq!(age, Some(30));
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
// 永不 panic 保障：禁止以下 panic-prone 模式
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

extern crate alloc;

// Kani 在 #![no_std] crate 中需要显式引入（Kani 工具链自动提供 kani crate）
#[cfg(kani)]
extern crate kani;

// 模块声明（按依赖顺序：基础类型在前）
pub mod domain;
pub mod error;
pub mod executor;
pub mod path;
pub mod transition;
pub mod value;

// Kani 形式化验证 proof（通过 #[path] 引入 src/，Kani 自动扫描 #[kani::proof] 函数）
// 与 evorule-reactor 的引入方式一致（pure.rs #[path] 引入 kani_proofs.rs）
#[cfg(kani)]
#[path = "../verification/kani_proofs.rs"]
mod kani_proofs;

// 核心类型重导出（仅公开稳定的 API）
pub use error::TcbError;
pub use transition::{execute_transition, TransitionResult, MAX_TRANSFORM_RULES};
pub use value::JsonValue;
