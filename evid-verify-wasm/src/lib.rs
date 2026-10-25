// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EVID 移动端验证器 wasm 绑定（判定逻辑单源复用 `evid-verify-core`）
//!
//! 双路线共享 [`handlers`] 判定层：
//!
//! - **route A（主产物）**：[`abi`] extern "C" 字节 ABI——入参 JSON 字节
//!   （JS 经 [`abi::wasm_alloc`] 写入线性内存），出参 `[u32 LE 长度][JSON]`
//!   包。零 import、零 JS 胶水，`WebAssembly.instantiate` 即用；单文件
//!   HTML 验证器（`dist/index.html`，经 `build-dist.ps1` 生成）走此路线。
//! - **route B**：`--features bindgen` 启用 [`bindings`]（`#[wasm_bindgen]`
//!   字符串接口），供 wasm-pack / npm 生态消费。
//!
//! # 判定面契约（三入口同构）
//!
//! 入参 JSON → verdict JSON（永不 panic，解析失败也是一条 fail-closed 红）：
//!
//! - 导出面：导出文档（裸形态或 `{"export": …}` 包装，仅 v1.0）→
//!   `{ok, verdicts, stopped_at, tail_hash, last_hash_matches}`
//! - WAL 面：WAL 行序列（`{"lines":[…]}` 或裸数组）→
//!   `{ok, verdicts, stopped_at, tail_hash}`
//! - 锚点面：`{"seals":[…], "keys":{key_id: 公钥 hex}}` →
//!   `{ok, seal_count}` 或 `{ok:false, error}`
//!
//! # 诚实边界
//!
//! 验证器判「链未被篡改」，不判「链首记的是真事」——真实性由签名锚点
//! 与门限签名体系承载；公钥信任根由使用方自行建立（HTML 界面手动粘贴
//! 建表，零硬编码）。

pub mod abi;
pub mod handlers;

#[cfg(feature = "bindgen")]
pub mod bindings;
