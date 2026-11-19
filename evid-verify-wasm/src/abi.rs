// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! route A: extern "C" byte ABI (zero imports, no JS glue)
//!
//! Calling contract (JS side):
//! 1. `const p = wasm_alloc(n)` allocates the input buffer → write n bytes of JSON;
//! 2. `const out = verify_export_wasm(p, n)` (or the wal/seals entries);
//! 3. `wasm_free(p, n)` returns the input; the output is a `[u32 LE length][JSON bytes]`
//!    package: length = `getUint32(out, true)`, then `wasm_free(out, 4 + len)` after reading.
//!
//! No path panics (workspace lints deny the panic family); null-pointer /
//! zero-length inputs return a structured error package — the validity
//! contract for non-null pointers is the caller's burden (the only legitimate
//! source is the [`wasm_alloc`] return value).

use crate::handlers;
use serde_json::{json, Value as Jv};

/// Allocate a len-byte buffer (capacity exactly len; for JS to write inputs, returned via [`wasm_free`] after use)
#[no_mangle]
pub extern "C" fn wasm_alloc(len: u32) -> *mut u8 {
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Return a buffer allocated by [`wasm_alloc`] (len must match the allocation)
///
/// # Safety
/// `ptr` must be a [`wasm_alloc`] return value with `len` matching the
/// allocation (null pointer / zero length is an explicit no-op); double frees
/// are a caller violation.
#[no_mangle]
pub unsafe extern "C" fn wasm_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: ptr comes from wasm_alloc (with_capacity(len) then forget); the
    // allocation layout is exactly len bytes. Rebuilding a zero-length Vec only
    // borrows its deallocation path (u8 has no drop semantics).
    unsafe {
        drop(Vec::from_raw_parts(ptr, 0, len as usize));
    }
}

/// Read-only borrow of the input bytes (does not free; freeing is the JS-side [`wasm_free`] duty)
///
/// # Safety
/// ptr must be a wasm_alloc return value with len bytes written (see the
/// module-level calling contract).
unsafe fn input_bytes<'a>(ptr: *const u8, len: u32) -> &'a [u8] {
    core::slice::from_raw_parts(ptr, len as usize)
}

/// Output package: `[u32 LE length][JSON bytes]` (capacity exactly 4+len, closing the loop with wasm_free)
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
        json!({ "ok": false, "error": "empty input" })
    } else {
        // SAFETY: contract in input_bytes (wasm_alloc allocation + len bytes written)
        let bytes = unsafe { input_bytes(ptr, len) };
        match serde_json::from_slice::<Jv>(bytes) {
            Ok(v) => match face {
                1 => handlers::handle_export(&v),
                2 => handlers::handle_wal(&v),
                _ => handlers::handle_seals(&v),
            },
            Err(e) => json!({ "ok": false, "error": format!("invalid input JSON: {e}") }),
        }
    };
    package(&verdict)
}

/// Export-surface verdict (input: export doc JSON bytes)
///
/// # Safety
/// `ptr` must be a [`wasm_alloc`] return value with `len` bytes of JSON
/// written (null pointer / zero length is rejected as a structured error
/// package, no UB); the input buffer is returned by the JS-side [`wasm_free`].
#[no_mangle]
pub unsafe extern "C" fn verify_export_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 1)
}

/// WAL-surface verdict (input: WAL line series JSON bytes)
///
/// # Safety
/// Same as [`verify_export_wasm`].
#[no_mangle]
pub unsafe extern "C" fn verify_wal_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 2)
}

/// Anchor-surface verdict (input: `{seals, keys}` JSON bytes)
///
/// # Safety
/// Same as [`verify_export_wasm`].
#[no_mangle]
pub unsafe extern "C" fn verify_seals_wasm(ptr: *const u8, len: u32) -> *mut u8 {
    dispatch(ptr, len, 3)
}
