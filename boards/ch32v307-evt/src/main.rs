// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! CH32V307VCT6 EVT board binary — bring-up skeleton (extended at
//! real-hardware bring-up)
//!
//! # Layout conventions (kept in sync with memory.x)
//! - Flash: 256KB @ `0x0800_0000`; the tail 8 pages × 4KB = 32KB
//!   (`0x0803_8000..0x0804_0000`) form the WAL tail-page ring, not part of
//!   any link REGION; the code REGION_TEXT/RODATA totals 224KB
//! - RAM: 64KB @ `0x2000_0000`
//! - Heap: TLSF static pool 8192 words = 32KB ([`TlsfGlobal`])
//!
//! # Run channel
//! probe-rs + defmt-rtt (RTT logs); the panic handler is a built-in minimal
//! one (print + halt).

#![no_std]
#![no_main]

use defmt_rtt as _;
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use riscv_rt::entry;

/// Kernel heap (TLSF static pool, 8192 words = 32 KiB)
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// Flash WAL ring region (tail 8 pages × 4KB = 32KB): the landing zone of the
/// write-ahead fact ledger. Programming is 1→0 only; the erase granularity
/// for the whole region is the 4KB page (consumed by the Flash WAL backend;
/// the ring layout starts with a tail-page ring of 8 pages, parameterized per
/// the evorule-mcu WAL backend conventions).
const WAL_REGION: core::ops::Range<usize> = 0x0803_8000..0x0804_0000;

// Fixed at 0 until there is a time base (replaced once the tick clock is
// wired up at real-hardware bring-up); defmt 1.x makes this a module-level
// function-style macro
defmt::timestamp!("{=u64}", 0);

/// Panic handling: report via defmt (including the location), then halt with wfi.
/// panic-probe is not used — it only supports Cortex-M (a compile_error on
/// non-thumb targets), so the RISC-V side carries its own minimal
/// implementation with the same semantics (print + halt).
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
    // SAFETY: static exclusive allocator; initialized exactly once at the
    // single entry point, before any allocation
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
