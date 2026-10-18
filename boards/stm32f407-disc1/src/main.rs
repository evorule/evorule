// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! STM32F407G-DISC1 板级二进制——bring-up 骨架（T1 实机批扩展）
//!
//! # 布局约定（memory.x 同步）
//! - Flash WAL 区段：sector0-3 = 4×16KB = 64KB @ `0x0800_0000..0x0801_0000`，
//!   不入任何链接 REGION；代码 REGION_FLASH 从 sector4（`0x0801_0000`）起
//! - RAM：主 SRAM 128KB（CCM 64KB 不用，DMA 不可达）
//! - 堆：TLSF 静态池 8192 字 = 32KB（[`TlsfGlobal`]）
//!
//! # 运行通道
//! probe-rs + defmt-rtt（RTT 日志）/ panic-probe（panic 打印 + halt）。

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use panic_probe as _;
use stm32f4xx_hal::{pac, prelude::*, rcc};

/// 内核堆（TLSF 静态池，8192 字 = 32 KiB）
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// Flash WAL 区段（sector0-3，64KB）：write-ahead 事实账本落点。
/// 编程只 1→0，整区段擦除粒度 = 16KB 扇区（T0.4 Flash 后端消费）。
const WAL_REGION: core::ops::Range<usize> = 0x0800_0000..0x0801_0000;

// 无时基期固定 0（T1 接 DWT/tick 时钟后替换）；defmt 1.x 起为模块级函数式宏
defmt::timestamp!("{=u64}", 0);

#[entry]
fn main() -> ! {
    // SAFETY: 静态独占分配器，entry 单入口恰一次初始化，先于任何分配
    unsafe { HEAP.init() };
    defmt::info!(
        "evorule f407-disc1 boot: tlsf ready (pool 32KiB), wal {:#08x}..{:#08x}",
        WAL_REGION.start as u32,
        WAL_REGION.end as u32
    );

    let dp = pac::Peripherals::take().unwrap();
    let _clocks = dp.RCC.constrain().freeze(rcc::Config::hse(8.MHz()));
    defmt::info!("clocks frozen: hse 8MHz");

    loop {
        cortex_m::asm::wfi();
    }
}
