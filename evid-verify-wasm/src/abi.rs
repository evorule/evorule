// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! route A：extern "C" 字节 ABI（零 import，免 JS 胶水）
//!
//! 调用契约（JS 侧）：
//! 1. `const p = wasm_alloc(n)` 分配入参缓冲 → 写入 n 字节 JSON；
//! 2. `const out = verify_export_wasm(p, n)`（或 wal/seals 入口）；
//! 3. `wasm_free(p, n)` 归还入参；出参为 `[u32 LE 长度][JSON 字节]` 包：
//!    长度 = `getUint32(out, true)`，读毕 `wasm_free(out, 4 + len)` 归还。
//!
//! 任何路径不 panic（workspace lints deny panic 族）；空指针/零长入参
//! 返回结构化错误包——非空指针的有效性契约由调用方承担（唯一合法来源
//! 是 [`wasm_alloc`] 返回值）。

use crate::handlers;
use serde_json::{json, Value as Jv};

/// 分配 len 字节缓冲（容量恰为 len；JS 写入入参用，用毕以 [`wasm_free`] 归还）
#[no_mangle]
pub extern "C" fn wasm_alloc(len: u32) -> *mut u8 {
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// 归还 [`wasm_alloc`] 分配的缓冲（len 必须与分配值一致）
///
/// # Safety
/// `ptr` 须为 [`wasm_alloc`] 返回且 `len` 与分配值一致（空指针/零长为
/// 显式无操作）；双重归还属调用方违约。
#[no_mangle]
pub unsafe extern "C" fn wasm_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: ptr 来自 wasm_alloc（with_capacity(len) 后 forget），分配布局恰
    // 为 len 字节；重建零长 Vec 仅借用其释放路径（u8 无 drop 语义）。
    unsafe {
        drop(Vec::from_raw_parts(ptr, 0, len as usize));
    }
}

/// 入参字节只读借用（不释放；释放责任在 JS 侧 [`wasm_free`]）
///
/// # Safety
/// ptr 须为 wasm_alloc 返回且已写入 len 字节（见模块级调用契约）。
unsafe fn input_bytes<'a>(ptr: *const u8, len: u32) -> &'a [u8] {
    core::slice::from_raw_parts(ptr, len as usize)
}

/// 出包：`[u32 LE 长度][JSON 字节]`（容量恰 4+len，与 wasm_free 闭环）
fn package(verdict: &Jv) -> *mut u8 {
    let body = verdict.to_string();
    let mut out: Vec<u8> = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body.as_bytes());
    let ptr = out.as_mut_ptr();
    core::mem::forget(out);
    ptr
}

fn dispatch(ptr: *const u8, len: u32, face: u8) -> *mut u8 {
    let verdict = if ptr.is_null() || len == 0 {
        json!({ "ok": false, "error": "空输入" })
    } else {
        // SAFETY: 契约见 input_bytes（wasm_alloc 分配 + 已写入 len 字节）
        let bytes = unsafe { input_bytes(ptr, len) };
        match serde_json::from_slice::<Jv>(bytes) {
            Ok(v) => match face {
                1 => handlers::handle_export(&v),
                2 => handlers::handle_wal(&v),
                _ => handlers::handle_seals(&v),
            },
            Err(e) => json!({ "ok": false, "error": format!("输入非法 JSON: {e}") }),
        }
    };
    package(&verdict)
}

/// 导出面判定（入参：导出文档 JSON 字节）
///
/// # Safety
/// `ptr` 须为 [`wasm_alloc`] 返回且已写入 `len` 字节 JSON（空指针/零长
/// 按结构化错误包拒收，不产生 UB）；入参缓冲由 JS 侧 [`wasm_free`] 归还。
#[no_mangle]
pub unsafe extern "C" fn verify_export_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 1)
}

/// WAL 面判定（入参：WAL 行序列 JSON 字节）
///
/// # Safety
/// 同 [`verify_export_wasm`]。
#[no_mangle]
pub unsafe extern "C" fn verify_wal_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 2)
}

/// 锚点面判定（入参：`{seals, keys}` JSON 字节）
///
/// # Safety
/// 同 [`verify_export_wasm`]。
#[no_mangle]
pub unsafe extern "C" fn verify_seals_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 3)
}
