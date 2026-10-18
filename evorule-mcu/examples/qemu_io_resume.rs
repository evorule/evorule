// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! QEMU mps2-an386 I/O 暂停-恢复样例：对拍 fixture 规则包 + 固定应答恢复
//!
//! # 对拍协议（与 `examples/native_io_resume_tail` 配对）
//! 同一 `.evopack` 规则包（双路径 I/O 循环规则）+ 同一指令 + 同一固定应答
//! 在 no_std 固件中执行完整暂停-恢复协议：run 触发 IoRequest 暂停 →
//! `IoBridge`（semihosting echo 面）执行 → `resume_io` 注入恢复 →
//! 消费到 `llm_response` 且 `__io_results__` 清除 → 输出
//! `IO_CHAIN_TAIL=<64 hex>`；与 native 参照输出逐字节一致即证 I/O 恢复
//! 协议跨目标一致（判据链见 native 侧文档）。
//!
//! 运行（与 qemu_parity 同编排，固件名替换为 qemu_io_resume）：
//! ```text
//! cargo build --release --target thumbv7em-none-eabihf \
//!     -p evorule-mcu --features qemu,tlsf-alloc --example qemu_io_resume
//! qemu-system-arm -machine mps2-an386 -nographic \
//!     -semihosting-config enable=on,target=native \
//!     -kernel <elf>
//! ```

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use evorule_mcu::{IoBridge, McuError, McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;
use panic_semihosting as _;

/// I/O 对拍 fixture（host 编译器 `compile-evopack` 产物，与 native 参照同一文件）
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/io_resume_rules.evopack");

/// 内核堆（TLSF 静态池，8192 字 = 32 KiB，与 parity 样例同容量）
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// 固定 LLM 应答（与 native 参照逐字段同值，对拍确定性）
fn canned_llm_result() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("role", JsonValue::string("assistant")),
        ("content", JsonValue::string("no tools needed")),
    ])
}

/// semihosting echo 面的确定性 canned 桥（仿真最小实现）：
/// 应答固定值进审计链，保证 QEMU 与 native 两侧应答逐字节一致
struct CannedBridge;

impl IoBridge for CannedBridge {
    fn perform(&mut self, io_type: &str, _params: &JsonValue) -> Result<JsonValue, String> {
        match io_type {
            "call_external" => Ok(canned_llm_result()),
            other => Err(format!("canned bridge: unsupported io_type {other}")),
        }
    }
}

/// fail-visible 退出（错误消息经 semihosting 留痕）
fn fail(msg: &str) -> ! {
    hprintln!("IO_PARITY_FAIL: {}", msg);
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

    // 指令：call_external（messages 透传，与 native 参照同值）
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "messages",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("role", JsonValue::string("user")),
                        ("content", JsonValue::string("hello from io parity")),
                    ])]),
                ),
                ("tools", JsonValue::array(vec![])),
            ]),
        ),
    ]);

    // run → IoRequest 入链后暂停
    let request_id = match ex.run(instruction) {
        Err(McuError::IoPaused {
            request_id,
            io_type,
        }) => {
            hprintln!("qemu paused: request_id={} io_type={}", request_id, io_type);
            if io_type != "call_external" {
                fail("unexpected io_type");
            }
            request_id
        }
        Ok(_) => fail("expected IoPaused, got Ok"),
        Err(e) => {
            let _ = format!("{e}"); // McuError: Display
            fail("run failed with unexpected error");
        }
    };

    // 板侧执行 I/O（canned 桥）后恢复
    let mut bridge = CannedBridge;
    let outcome = match ex.pending_ios().first() {
        Some(p) if p.request_id == request_id => bridge.perform(&p.io_type, &p.params),
        _ => fail("pending io missing or mismatched"),
    };
    match ex.resume_io(request_id, outcome) {
        Ok(report) => hprintln!(
            "qemu resumed: steps={} ticks={} version={}",
            report.steps_executed,
            report.ticks,
            report.version
        ),
        Err(_) => fail("resume failed"),
    }

    // 场景终态自检：llm_response 消费到位，容器清除不残留
    match ex
        .payload()
        .get("llm_response")
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
    {
        Some("no tools needed") => {}
        _ => fail("llm_response.content mismatch"),
    }
    if ex.payload().get("__io_results__").is_some() {
        fail("__io_results__ container not cleared");
    }

    hprintln!("IO_CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
