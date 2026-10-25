// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! route B：`#[wasm_bindgen]` 绑定面（wasm-pack / npm 生态）
//!
//! 以 `--features bindgen` 启用（不进默认构建——wasm-bindgen 会往模块里
//! 挂宿主 import，破坏 route A 的零 import 免安装实例化）。宿主（非
//! wasm32）目标同样可编译可测；判定逻辑同 [`crate::handlers`]，字符串进出。

use serde_json::{json, Value as Jv};
use wasm_bindgen::prelude::*;

use crate::handlers;

fn run(input: &str, f: impl Fn(&Jv) -> Jv) -> String {
    match serde_json::from_str::<Jv>(input) {
        Ok(v) => f(&v).to_string(),
        Err(e) => json!({ "ok": false, "error": format!("输入非法 JSON: {e}") }).to_string(),
    }
}

/// 导出面判定（JSON 字符串进出）
#[wasm_bindgen]
pub fn verify_export_json(input: &str) -> String {
    run(input, handlers::handle_export)
}

/// WAL 面判定（JSON 字符串进出）
#[wasm_bindgen]
pub fn verify_wal_json(input: &str) -> String {
    run(input, handlers::handle_wal)
}

/// 锚点面判定（JSON 字符串进出）
#[wasm_bindgen]
pub fn verify_seals_json(input: &str) -> String {
    run(input, handlers::handle_seals)
}
