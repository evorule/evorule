// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TLSF global allocator wrapper — supplies `#[global_allocator]` for board binaries
//!
//! # Positioning
//! A minimal `GlobalAlloc` wrapper over the [`tlsf`] crate (Two-Level
//! Segregated Fit, bounded constant-time O(1) allocation). The tlsf author
//! intentionally does not ship a built-in `GlobalAlloc` implementation (the
//! synchronization policy and the realloc trade-off are application-side
//! decisions, see its README "Rejected features") — this module is the
//! project-side decision for that application choice:
//!
//! - **Synchronization decision**: single-core single-threaded runtime;
//!   interrupt service routines must not touch the heap (enforced as a usage
//!   discipline, not by locking) — zero locks, zero overhead
//! - **realloc decision**: use the [`GlobalAlloc`] default implementation
//!   (alloc + copy + dealloc); the worst-case O(N) is inherent to memcpy
//! - **Pool shape**: a static word array ([`POOL_WORDS`] u32 words), the same
//!   shape as the pool unit of [`tlsf::Tlsf::initialize`]; the allocator and
//!   the pool live in one `static`, and the self-referential borrow presumes
//!   `'static` storage
//!
//! # Capacity parameters (FLL=11)
//! - Per-allocation cap 62,488 B (beyond it OOM → null →
//!   `handle_alloc_error` abort)
//! - Allocator header overhead 376 B, 4/8 B per block
//! - Pool total = `POOL_WORDS * 4` B; must match the heap section of the
//!   board-level memory.x
//!
//! # Usage (board binary)
//! ```ignore
//! use evorule_mcu::alloc_tlsf::TlsfGlobal;
//!
//! #[global_allocator]
//! static HEAP: TlsfGlobal<8192> = TlsfGlobal::new(); // 32 KiB pool
//!
//! #[entry]
//! fn main() -> ! {
//!     unsafe { HEAP.init() }; // exactly once, before any allocation
//!     // ...
//! }
//! ```
//!
//! # Gating
//! Gated behind the `tlsf-alloc` feature (optional dependency): the kernel
//! proper and host default builds do not pull it in; board binaries and
//! simulation examples enable it explicitly. This module is target-agnostic
//! (no target cfg); host-side functional tests directly cover the
//! alloc/dealloc/alignment/OOM semantics.
//!
//! [`POOL_WORDS`]: TlsfGlobal

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};

use tlsf::Tlsf;

/// First-list length: FLL=11 → per-allocation cap 62,488 B, header overhead
/// 376 B (see module docs)
const FLL: usize = 11;

/// TLSF global allocator (single-threaded, lock-free form)
///
/// Generic parameter `POOL_WORDS`: number of static pool words (u32 units);
/// pool total = the value × 4 B. Const construction + one-time
/// [`TlsfGlobal::init`] initialization, fitting the static allocator
/// declaration form.
pub struct TlsfGlobal<const POOL_WORDS: usize> {
    tlsf: UnsafeCell<MaybeUninit<Tlsf<'static, FLL>>>,
    pool: UnsafeCell<MaybeUninit<[MaybeUninit<u32>; POOL_WORDS]>>,
}

impl<const POOL_WORDS: usize> Default for TlsfGlobal<POOL_WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const POOL_WORDS: usize> TlsfGlobal<POOL_WORDS> {
    /// Const construction (uninitialized; [`TlsfGlobal::init`] must be called
    /// exactly once before the first allocation)
    pub const fn new() -> Self {
        Self {
            tlsf: UnsafeCell::new(MaybeUninit::uninit()),
            pool: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Initialize the allocator with its own static pool
    ///
    /// # Safety
    /// - Must be called exactly once, in a single-threaded context (the board
    ///   startup entry) before the first allocation
    /// - `Self` must live in `'static` storage (a `static` item) — the
    ///   `'static` lifetime of the pool borrow holds under that premise
    pub unsafe fn init(&'static self) {
        // SAFETY: 'static self + single call → the pool's exclusive mutable borrow holds
        let pool: &mut [MaybeUninit<u32>] = unsafe {
            let base = (*self.pool.get()).as_mut_ptr();
            core::slice::from_raw_parts_mut(base.cast::<MaybeUninit<u32>>(), POOL_WORDS)
        };
        // SAFETY: 'static self + single call → exclusive write access to the tlsf field holds
        unsafe {
            (*self.tlsf.get()).write(Tlsf::empty());
            (*self.tlsf.get()).assume_init_mut().initialize(pool);
        }
    }
}

// SAFETY: see the synchronization decision in the module docs — single-threaded
// runtime + the ISR-must-not-touch-heap usage discipline; no concurrent access
// surface, so no lock is needed
unsafe impl<const POOL_WORDS: usize> Sync for TlsfGlobal<POOL_WORDS> {}

unsafe impl<const POOL_WORDS: usize> GlobalAlloc for TlsfGlobal<POOL_WORDS> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: init precedes all allocations (init contract); single-threaded, no concurrent access
        let tlsf = unsafe { (*self.tlsf.get()).assume_init_mut() };
        match tlsf.memalign(layout) {
            Some(block) => block.as_mut_ptr().cast::<u8>(),
            None => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: the GlobalAlloc contract guarantees ptr is the start of a
        // block returned by alloc; tlsf free derives the size from the block
        // header, so no layout is needed
        let tlsf = unsafe { (*self.tlsf.get()).assume_init_mut() };
        tlsf.free(unsafe { NonNull::new_unchecked(ptr.cast::<u32>()) });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;

    #[test]
    fn alloc_dealloc_roundtrip() {
        static HEAP: TlsfGlobal<4096> = TlsfGlobal::new(); // 16 KiB pool
        unsafe { HEAP.init() };
        unsafe {
            let layout = Layout::from_size_align(100, 4).unwrap();
            let p = HEAP.alloc(layout);
            assert!(!p.is_null());
            core::ptr::write_bytes(p, 0xAB, 100);
            HEAP.dealloc(p, layout);
        }
    }

    #[test]
    fn alignment_satisfied() {
        static HEAP: TlsfGlobal<4096> = TlsfGlobal::new();
        unsafe { HEAP.init() };
        for align in [4, 8, 16, 32] {
            unsafe {
                let layout = Layout::from_size_align(64, align).unwrap();
                let p = HEAP.alloc(layout);
                assert!(!p.is_null(), "align {align} alloc failed");
                assert_eq!(p as usize % align, 0, "align {align} not satisfied");
                HEAP.dealloc(p, layout);
            }
        }
    }

    #[test]
    fn multi_alloc_free_interleaved() {
        static HEAP: TlsfGlobal<4096> = TlsfGlobal::new();
        unsafe { HEAP.init() };
        unsafe {
            let mut ptrs = [core::ptr::null_mut::<u8>(); 8];
            for (i, slot) in ptrs.iter_mut().enumerate() {
                let size = 16 * (i + 1);
                let layout = Layout::from_size_align(size, 4).unwrap();
                let p = HEAP.alloc(layout);
                assert!(!p.is_null());
                core::ptr::write_bytes(p, i as u8, size);
                *slot = p;
            }
            // Free in reverse order (same shape as the upstream tlsf tests)
            for (i, p) in ptrs.iter().enumerate().rev() {
                let layout = Layout::from_size_align(16 * (i + 1), 4).unwrap();
                HEAP.dealloc(*p, layout);
            }
            // After freeing, the pool should be allocatable again
            let layout = Layout::from_size_align(512, 4).unwrap();
            let p = HEAP.alloc(layout);
            assert!(!p.is_null(), "post-free realloc failed");
            HEAP.dealloc(p, layout);
        }
    }

    #[test]
    fn oom_returns_null() {
        static HEAP: TlsfGlobal<64> = TlsfGlobal::new(); // 256 B pool
        unsafe { HEAP.init() };
        unsafe {
            // A single request larger than the pool → null
            let layout = Layout::from_size_align(1024, 4).unwrap();
            assert!(HEAP.alloc(layout).is_null());
            // Allocate until exhaustion → null
            let layout = Layout::from_size_align(100, 4).unwrap();
            let a = HEAP.alloc(layout);
            assert!(!a.is_null());
            let b = HEAP.alloc(layout);
            assert!(!b.is_null());
            assert!(HEAP.alloc(layout).is_null(), "expected OOM");
            HEAP.dealloc(a, layout);
            HEAP.dealloc(b, layout);
        }
    }

    #[test]
    fn zero_sized_alloc_ok() {
        static HEAP: TlsfGlobal<64> = TlsfGlobal::new();
        unsafe { HEAP.init() };
        unsafe {
            let layout = Layout::from_size_align(0, 4).unwrap();
            let p = HEAP.alloc(layout);
            // A zero-sized request may return null (GlobalAlloc contract) or a unique non-null pointer
            if !p.is_null() {
                HEAP.dealloc(p, layout);
            }
        }
    }
}
