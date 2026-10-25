// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EVID 移动端审计链独立验证内核（no_std）
//!
//! 零信任判定内核：给定审计链数据（S1 导出文档 / WAL 行序列 / 锚点封条链），
//! 从公式重算完成完整性判定，不信任任何外部结论。供三方共用：
//! wasm 绑定（移动端单文件 HTML 验证器）、server 侧复核、哨兵侧对照。
//!
//! # 三面判定
//!
//! - **导出面**（[`verify::verify_export`]，四步/条）：prev 链接 → 内容哈希
//!   （`fact_hash` 公式重算）→ stable 深比（`fact_to_stable_json` 重算与
//!   `fact_json_stable` 原文对账 + 条目投影一致性）→ 链折叠；链尾对
//!   `last_hash`。
//! - **WAL 面**（[`verify::verify_wal_lines`]，三步/行）：prev 链接 →
//!   `content_hash` 重算 → `chain_hash` 链折叠。
//! - **锚点面**（[`seal::verify_seal_chain`]，四规则）：seq 连续 → 区间
//!   无缝 → anchor_hash 链 → ed25519 验签（v2 载荷语义）。
//!
//! # 公式纪律
//!
//! `fact_hash = blake3(to_string(fact_to_stable_json(fact)))`；
//! `chain_step = blake3(prev_hex ++ content_hex)`，链首 prev = `genesis`；
//! `anchor_hash = blake3(prev_anchor ++ '|' ++ 载荷九字段)`，锚点链首
//! prev = `anchor-genesis`。事实链公式全部经 [`evorule_audit_chain::hash`]
//! SSOT 调用，本 crate 不私写事实链哈希路径。
//!
//! # fail-closed 判形
//!
//! - 导出版本仅认 `1.0`，锚点仅认 v2 载荷语义（`evorule-anchor/2`，
//!   0.8.1 的 v1 锚点不兼容，明示诚实边界）；
//! - 未知 Fact 变体 / 缺字段 / 类型不符 / 浮点数字段一律拒收；
//! - 数字通道与 SSOT 序列化对偶：i64 全域 → `Integer`，u64 高位 →
//!   `U64`，浮点拒收（u64 全域纪律）。
//!
//! # 诚实边界
//!
//! - 本内核判「链未被篡改」，不判「链首记的是真事」（那是签名锚点与
//!   T3 门限签名的领域）；
//! - JSON 数字无形态标签，`U64(5)` 解析 roundtrip 后归位 `Integer(5)`
//!   （值域保真；两形态 stable 字节等价，不影响哈希判定）。
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod parse;
pub mod record;
pub mod seal;
pub mod verify;
