// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! S4 移动端黄金向量闸（宿主侧/正本）：对 dist/fixtures 冻结向量跑 handlers
//! 判定层（core 同源 = 正本），输出落盘供 Node 侧与 wasm32 产物逐字节对拍。
//!
//! `EVID_WASM_HOST_OUT_DIR` 置位才落盘（默认无操作——CI 常态与本地全量
//! 测试不受影响；对拍脚本见 scripts/evid_wasm_gate.mjs，闸位见 wasm.yml）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_wasm::handlers;
use serde_json::Value as Jv;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dist/fixtures");

/// fixture 文件名 → 判定面（与 abi.rs dispatch face 对应；1=导出 2=WAL 3=锚点）
const FACES: &[(&str, u8)] = &[
    ("export_valid", 1),
    ("export_tampered", 1),
    ("self_check", 1),
    ("wal_valid", 2),
    ("wal_tampered", 2),
    ("seals_valid", 3),
    ("seals_tampered", 3),
];

fn run_face(face: u8, input: &Jv) -> Jv {
    match face {
        1 => handlers::handle_export(input),
        2 => handlers::handle_wal(input),
        _ => handlers::handle_seals(input),
    }
}

#[test]
fn dump_host_expected_outputs() {
    let Some(dir) = std::env::var_os("EVID_WASM_HOST_OUT_DIR") else {
        return; // 默认无操作（CI 常态），置位才落盘
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("输出目录应可创建");
    for (name, face) in FACES {
        let raw = std::fs::read(format!("{FIXTURES}/{name}.json")).expect("冻结向量应存在");
        let input: Jv = serde_json::from_slice(&raw).expect("冻结向量应可解析");
        let verdict = run_face(*face, &input);
        // 极性自证：valid/self_check 必绿、tampered 必红（fixture 本身可信先证）
        let expect_ok = !name.contains("tampered");
        assert_eq!(verdict["ok"], Jv::Bool(expect_ok), "{name} 极性不符");
        // 紧凑序列化逐字节落盘（与 abi.rs package 出包同式：verdict.to_string()）
        std::fs::write(dir.join(format!("{name}.out.json")), verdict.to_string())
            .expect("期望输出写入失败");
    }
}
