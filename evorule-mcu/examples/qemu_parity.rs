// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! QEMU mps2-an386 最小内核样例：对拍 fixture 规则包 + 同场景链尾哈希
//!
//! # 对拍协议（与 `examples/native_parity_tail` 配对）
//! 同一 `.evopack` 规则包 + 同一指令序列（increment(x,5) → noop →
//! increment(x,-2)）在 no_std 固件中执行，semihosting 输出逐 run 报告与
//! 最终 `CHAIN_TAIL=<64 hex>`；与 native 参照输出逐字节一致即证链尾哈希
//! 跨目标一致（判据链见 native 侧文档）。
//!
//! 运行（scripts/qemu_parity.ps1 编排）：
//! ```text
//! cargo build --release --target thumbv7em-none-eabihf \
//!     -p evorule-mcu --features qemu --example qemu_parity
//! qemu-system-arm -machine mps2-an386 -nographic \
//!     -semihosting-config enable=on,target=native \
//!     -kernel <elf>
//! ```

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use evorule_mcu::{McuExecutor, RulePack};
use evorule_tcb::JsonValue;
use panic_semihosting as _;

/// 对拍 fixture（host 编译器 `compile-evopack` 产物，与 native 参照同一文件）
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/parity_rules.evopack");

/// 内核堆（TLSF 静态池，8192 字 = 32 KiB，与对拍场景原容量一致）
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

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

/// fail-visible 退出（错误消息经 semihosting 留痕）
fn fail(msg: &str) -> ! {
    hprintln!("PARITY_FAIL: {}", msg);
    debug::exit(debug::EXIT_FAILURE);
    loop {
        core::hint::spin_loop();
    }
}

#[entry]
fn main() -> ! {
    // SAFETY: HEAP 为静态独占分配器，初始化恰好在 main 入口执行一次
    // （entry 单入口语义），且中断在 core 启动后、init 前不会触碰堆
    unsafe {
        HEAP.init();
    }

    let pack = match RulePack::from_evopack(EVOPACK) {
        Ok(p) => p,
        Err(_) => fail("fixture evopack decode failed"),
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
            Ok(report) => hprintln!(
                "qemu run {}: steps={} ticks={} version={}",
                i + 1,
                report.steps_executed,
                report.ticks,
                report.version
            ),
            Err(_) => fail("run failed"),
        }
    }

    // 场景终态自检：x = 5 - 2 = 3
    match ex.payload().get("x").and_then(|v| v.as_i64()) {
        Some(3) => {}
        _ => fail("payload x expected 3"),
    }

    hprintln!("CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
