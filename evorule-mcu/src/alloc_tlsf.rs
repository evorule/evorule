// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TLSF 全局分配器包装——板级二进制的 `#[global_allocator]` 供给
//!
//! # 定位
//! [`tlsf`] crate（Two-Level Segregated Fit，有界常数时间 O(1) 分配）的
//! 最小 `GlobalAlloc` 包装。tlsf 作者有意不内置 `GlobalAlloc` 实现
//! （同步策略与 realloc 取舍属应用侧决策，见其 README "Rejected
//! features"）——本模块即该应用侧决策的项目内定案：
//!
//! - **同步决策**：单核单线程运行时，中断服务程序禁止触碰堆（使用纪律
//!   约束，非锁保护）——零锁零开销
//! - **realloc 决策**：走 [`GlobalAlloc`] 缺省实现（alloc + copy + dealloc），
//!   最坏 O(N) 为 memcpy 固有成本
//! - **池形态**：静态字数组（[`POOL_WORDS`] 个 u32），与
//!   [`tlsf::Tlsf::initialize`] 的池单元同型；分配器与池同置一个
//!   `static`，自引用借用以 `'static` 存储为前提
//!
//! # 容量参数（FLL=11）
//! - 单次分配上限 62,488 B（超出即 OOM→null→`handle_alloc_error` abort）
//! - 分配器头开销 376 B，每块 4/8 B
//! - 池总量 = `POOL_WORDS * 4` B，须匹配板级 memory.x 的堆区段
//!
//! # 用法（板级二进制）
//! ```ignore
//! use evorule_mcu::alloc_tlsf::TlsfGlobal;
//!
//! #[global_allocator]
//! static HEAP: TlsfGlobal<8192> = TlsfGlobal::new(); // 32 KiB 池
//!
//! #[entry]
//! fn main() -> ! {
//!     unsafe { HEAP.init() }; // 恰一次，先于任何分配
//!     // ...
//! }
//! ```
//!
//! # 门控
//! `tlsf-alloc` feature 门控（可选依赖）：内核本体与 host 默认构建不拉入；
//! 板级二进制与仿真样例显式开启。本模块目标无关（无 target cfg），
//! host 侧功能测试直接覆盖分配/释放/对齐/OOM 语义。
//!
//! [`POOL_WORDS`]: TlsfGlobal

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};

use tlsf::Tlsf;

/// 第一级列表长度：FLL=11 → 单次分配上限 62,488 B、头开销 376 B（见模块文档）
const FLL: usize = 11;

/// TLSF 全局分配器（单线程零锁形态）
///
/// 泛型参数 `POOL_WORDS`：静态池字数（u32 单位），池总量 = 该值 × 4 B。
/// 常量构造 + [`TlsfGlobal::init`] 一次性初始化，适配静态分配器声明形态。
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
    /// 常量构造（未初始化；首次分配前必须恰一次 [`TlsfGlobal::init`]）
    pub const fn new() -> Self {
        Self {
            tlsf: UnsafeCell::new(MaybeUninit::uninit()),
            pool: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// 以自带静态池初始化分配器
    ///
    /// # Safety
    /// - 必须在首次分配前的单线程上下文（板级启动入口）恰好调用一次
    /// - `Self` 必须位于 `'static` 存储（`static` 项）——池借用的
    ///   `'static` 生命周期以该前提成立
    pub unsafe fn init(&'static self) {
        // SAFETY: 'static self + 单次调用 → 池独占可变借用成立
        let pool: &mut [MaybeUninit<u32>] = unsafe {
            let base = (*self.pool.get()).as_mut_ptr();
            core::slice::from_raw_parts_mut(base.cast::<MaybeUninit<u32>>(), POOL_WORDS)
        };
        // SAFETY: 'static self + 单次调用 → tlsf 字段独占写入成立
        unsafe {
            (*self.tlsf.get()).write(Tlsf::empty());
            (*self.tlsf.get()).assume_init_mut().initialize(pool);
        }
    }
}

// SAFETY: 见模块文档同步决策——单线程运行时 + ISR 禁触碰堆的使用纪律，
// 无并发访问面，故无需锁
unsafe impl<const POOL_WORDS: usize> Sync for TlsfGlobal<POOL_WORDS> {}

unsafe impl<const POOL_WORDS: usize> GlobalAlloc for TlsfGlobal<POOL_WORDS> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: init 先于一切分配（init 契约）；单线程无并发访问
        let tlsf = unsafe { (*self.tlsf.get()).assume_init_mut() };
        match tlsf.memalign(layout) {
            Some(block) => block.as_mut_ptr().cast::<u8>(),
            None => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: GlobalAlloc 契约保证 ptr 为 alloc 返回的块起点；
        // tlsf free 依块头自导大小，无需 layout
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
        static HEAP: TlsfGlobal<4096> = TlsfGlobal::new(); // 16 KiB 池
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
            // 逆序释放（与 tlsf 上游测试同型）
            for (i, p) in ptrs.iter().enumerate().rev() {
                let layout = Layout::from_size_align(16 * (i + 1), 4).unwrap();
                HEAP.dealloc(*p, layout);
            }
            // 释放后池应可再分配
            let layout = Layout::from_size_align(512, 4).unwrap();
            let p = HEAP.alloc(layout);
            assert!(!p.is_null(), "post-free realloc failed");
            HEAP.dealloc(p, layout);
        }
    }

    #[test]
    fn oom_returns_null() {
        static HEAP: TlsfGlobal<64> = TlsfGlobal::new(); // 256 B 池
        unsafe { HEAP.init() };
        unsafe {
            // 单次超池请求 → null
            let layout = Layout::from_size_align(1024, 4).unwrap();
            assert!(HEAP.alloc(layout).is_null());
            // 分配至耗尽 → null
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
            // 零尺寸请求允许 null（GlobalAlloc 契约）或唯一非空指针
            if !p.is_null() {
                HEAP.dealloc(p, layout);
            }
        }
    }
}
