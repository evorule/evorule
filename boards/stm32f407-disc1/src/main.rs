// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! STM32F407G-DISC1 board binary — bring-up skeleton (extended at
//! real-hardware bring-up)
//!
//! # Layout conventions (kept in sync with memory.x)
//! - Flash WAL region: sectors 0-3 = 4×16KB = 64KB @
//!   `0x0800_0000..0x0801_0000`, not part of any link REGION; the code
//!   REGION_FLASH starts at sector 4 (`0x0801_0000`)
//! - RAM: main SRAM 128KB (the 64KB CCM is unused, unreachable by DMA)
//! - Heap: TLSF static pool 8192 words = 32KB ([`TlsfGlobal`])
//!
//! # Run channel
//! probe-rs + defmt-rtt (RTT logs) / panic-probe (panic printing + halt).

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use panic_probe as _;
use stm32f4xx_hal::{pac, prelude::*, rcc};

/// Kernel heap (TLSF static pool, 8192 words = 32 KiB)
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// Flash WAL region (sectors 0-3, 64KB): the landing zone of the write-ahead
/// fact ledger. Programming is 1→0 only; the erase granularity for the whole
/// region is the 16KB sector (consumed by the Flash WAL backend).
const WAL_REGION: core::ops::Range<usize> = 0x0800_0000..0x0801_0000;

// Fixed at 0 until there is a time base (replaced once the DWT/tick clock is
// wired up at real-hardware bring-up); defmt 1.x makes this a module-level
// function-style macro
defmt::timestamp!("{=u64}", 0);

#[entry]
fn main() -> ! {
    // SAFETY: static exclusive allocator; initialized exactly once at the
    // single entry point, before any allocation
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
