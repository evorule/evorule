// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! native 参照：同场景 host 编译的链尾哈希（仿真对拍基准值）
//!
//! # 判据链（仿真确定性对拍）
//! 1. T3 对拍单测（`exec::tests`）已证：同输入下 host 内核与 reactor std
//!    宿主全链逐条 WAL 字节级一致、链尾哈希一致
//! 2. 本例打印同场景 host 内核链尾（基准值）
//! 3. `examples/qemu_parity`（mps2-an386 固件）输出与之逐字节一致
//!    → 跨目标确定性成立，传递闭合 std 宿主等价
//!
//! # 场景（与 T3 多指令对拍单测同构）
//! parity fixture 规则包（`tests/fixtures/parity_rules.evopack`）+
//! 三条指令：increment(x,5) → noop → increment(x,-2)；期望终态 x = 3。
//!
//! 运行：`cargo run -p evorule-mcu --example native_parity_tail`
//! 输出：`CHAIN_TAIL=<64 hex>`（stdout），进度走 stderr。

use std::process::ExitCode;

use evorule_mcu::{McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;

/// 对拍 fixture（host 编译器产物，与 QEMU 固件 include 的同一文件）
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/parity_rules.evopack");

fn increment_instruction(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn noop_instruction() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

fn main() -> ExitCode {
    let pack = match RulePack::from_evopack(EVOPACK) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("E: fixture evopack decode failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut ex = McuExecutor::new(pack);

    for (i, instr) in [
        increment_instruction("x", 5),
        noop_instruction(),
        increment_instruction("x", -2),
    ]
    .into_iter()
    .enumerate()
    {
        match ex.run(instr) {
            Ok(report) => eprintln!(
                "native run {}: steps={} ticks={} version={}",
                i + 1,
                report.steps_executed,
                report.ticks,
                report.version
            ),
            Err(e) => {
                eprintln!("E: run {} failed: {e}", i + 1);
                return ExitCode::FAILURE;
            }
        }
    }

    // 场景终态自检：x = 5 - 2 = 3（与 T3 对拍单测一致）
    match ex.payload().get("x").and_then(|v| v.as_i64()) {
        Some(3) => {}
        other => {
            eprintln!("E: payload x expected 3, got {other:?}");
            return ExitCode::FAILURE;
        }
    }

    println!("CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    ExitCode::SUCCESS
}
