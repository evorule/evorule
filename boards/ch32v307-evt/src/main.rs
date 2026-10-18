// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! CH32V307VCT6 EVT 板级二进制——bring-up 骨架（实机批扩展）
//!
//! # 布局约定（memory.x 同步）
//! - Flash：256KB @ `0x0800_0000`；尾部 8 页 × 4KB = 32KB（
//!   `0x0803_8000..0x0804_0000`）为 WAL 尾部页环，不入任何链接 REGION；
//!   代码 REGION_TEXT/RODATA 合计 224KB
//! - RAM：64KB @ `0x2000_0000`
//! - 堆：TLSF 静态池 8192 字 = 32KB（[`TlsfGlobal`]）
//!
//! # 运行通道
//! probe-rs + defmt-rtt（RTT 日志）；panic 自带最小 handler（打印 + halt）。

#![no_std]
#![no_main]

use defmt_rtt as _;
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use riscv_rt::entry;

/// 内核堆（TLSF 静态池，8192 字 = 32 KiB）
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// Flash WAL 环区段（尾部 8 页 × 4KB = 32KB）：write-ahead 事实账本落点。
/// 编程只 1→0，整区段擦除粒度 = 4KB 页（Flash WAL 后端消费；环形布局
/// 尾部页环 8 页起步，参数化见 evorule-mcu wal 后端约定）。
const WAL_REGION: core::ops::Range<usize> = 0x0803_8000..0x0804_0000;

// 无时基期固定 0（实机批接 tick 时钟后替换）；defmt 1.x 起为模块级函数式宏
defmt::timestamp!("{=u64}", 0);

/// panic 处理：defmt 报错（含位置）后 wfi 停机。
/// 不用 panic-probe——其仅支持 Cortex-M（非 thumb 目标直接 compile_error），
/// RISC-V 侧按同语义（打印 + 停机）自带最小实现。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("PANIC");
    if let Some(loc) = info.location() {
        defmt::error!("at {}:{}", loc.file(), loc.line() as u32);
    }
    loop {
        riscv::asm::wfi();
    }
}

#[entry]
fn main() -> ! {
    // SAFETY: 静态独占分配器，entry 单入口恰一次初始化，先于任何分配
    unsafe { HEAP.init() };
    defmt::info!(
        "evorule ch32v307-evt boot: tlsf ready (pool 32KiB), wal {:#08x}..{:#08x}",
        WAL_REGION.start as u32,
        WAL_REGION.end as u32
    );

    loop {
        riscv::asm::wfi();
    }
}
