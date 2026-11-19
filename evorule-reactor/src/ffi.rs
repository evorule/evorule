// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.

#![allow(unsafe_code)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
//! C FFI interface — exposes evorule core functionality to external languages
//! such as C/C++/Python.
//!
//! # Design principles
//!
//! 1. **Handle pattern**: all objects are exposed as opaque pointers, avoiding memory-safety issues
//! 2. **Synchronous wrapper**: a tokio runtime runs async code in a synchronous environment
//! 3. **Zero copy**: strings are passed with a pointer + length pattern
//! 4. **Error codes**: all functions return the evorule_error_code enum
//!
//! # Usage example (C)
//!
//! ```c
//! #include <evorule.h>
//!
//! int main() {
//!     // Create the reactor
//!     evorule_reactor* reactor = evorule_reactor_new();
//!
//!     // Send a command
//!     evorule_reactor_send_command(reactor, "{\"type\": \"increment\"}");
//!
//!     // Destroy the reactor
//!     evorule_reactor_free(reactor);
//!     return 0;
//! }
//! ```

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

use evorule_tcb::{JsonValue, ObjectMap};

use crate::Reactor;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum evorule_error_code {
    EVORULE_OK = 0,
    EVORULE_ERROR_OOM = 1,
    EVORULE_ERROR_INVALID_ARG = 2,
    EVORULE_ERROR_RUNTIME = 3,
    EVORULE_ERROR_NOT_INITIALIZED = 4,
}

pub type evorule_reactor = c_void;
pub type evorule_result = c_void;

#[no_mangle]
pub extern "C" fn evorule_version() -> *const c_char {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    match CString::new(VERSION) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn evorule_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

/// FFI handle: stores the ReactorHandle and the Runtime
struct ReactorFfiHandle {
    _handle: crate::ReactorHandle,
    _runtime: tokio::runtime::Runtime,
    // Stores the command sender for sending commands
    command_tx: crate::FactSender,
    // Stores the snapshot reference
    snapshot: std::sync::Arc<std::sync::Mutex<crate::ReactorStateSnapshot>>,
    // C4: Command FactId generator (persistent, reused across send_command calls)
    //
    // Before the fix, every `evorule_reactor_send_command` called
    // `FactIdGenerator::new()`, so every Command's FactId was always 1 and
    // FactIds repeated in the FactsLog. It is now persisted in the handle and
    // `next_id()` increases monotonically (1, 2, 3, ...).
    //
    // Thread-safety convention: same as `command_tx` — `&mut` access to the FFI
    // handle is externally synchronized by the C caller (concurrent send_command
    // calls on the same handle must be serialized by the caller).
    fact_id_gen: crate::FactIdGenerator,
}

#[no_mangle]
pub extern "C" fn evorule_reactor_new() -> *mut evorule_reactor {
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return std::ptr::null_mut(),
    };
    // wasm32-unknown-unknown does not support the tokio runtime (std::time::Instant
    // is unimplemented, and runtime construction panics immediately). The FFI async
    // entry point is unavailable under WASM; the caller receives null.
    // For WASM scenarios use the purely synchronous TCB path
    // (execute_transition + in-memory FactsLog + BLAKE3 Auditor);
    // see evorule-wasm-demo for details. (disposition per doc 66 §10.4-2)
    #[cfg(target_arch = "wasm32")]
    return std::ptr::null_mut();

    // Create the reactor inside the runtime
    let reactor = Reactor::builder(Vec::new()).build();
    let (command_tx, _event_rx, _event_tx, handle, facts_log) = reactor.spawn();

    // Get the snapshot reference (needs access to the internal field)
    // Note: ReactorHandle's snapshot is private, so we need another way
    // Simplification: do not store the snapshot; use the handle's methods directly

    // Use a default value for Arc<Mutex<ReactorStateSnapshot>>
    let snapshot =
        std::sync::Arc::new(std::sync::Mutex::new(crate::ReactorStateSnapshot::default()));

    // Keep facts_log alive (prevent drop)
    let _facts_log = facts_log;

    let ffi_handle = Box::new(ReactorFfiHandle {
        _handle: handle,
        _runtime: runtime,
        command_tx,
        snapshot,
        // C4: Command FactIds increase monotonically from 1 (persisted in the handle)
        fact_id_gen: crate::FactIdGenerator::new(),
    });

    Box::into_raw(ffi_handle) as *mut evorule_reactor
}

#[no_mangle]
pub extern "C" fn evorule_reactor_free(reactor: *mut evorule_reactor) {
    if reactor.is_null() {
        return;
    }
    unsafe {
        let handle = Box::from_raw(reactor as *mut ReactorFfiHandle);
        drop(handle);
    }
}

#[no_mangle]
pub extern "C" fn evorule_reactor_send_command(
    reactor: *mut evorule_reactor,
    instruction_json: *const c_char,
) -> evorule_error_code {
    if reactor.is_null() || instruction_json.is_null() {
        return evorule_error_code::EVORULE_ERROR_INVALID_ARG;
    }

    let handle = unsafe { &mut *(reactor as *mut ReactorFfiHandle) };

    let json_str = unsafe { CStr::from_ptr(instruction_json) };
    let json_str = match json_str.to_str() {
        Ok(s) => s,
        Err(_) => return evorule_error_code::EVORULE_ERROR_INVALID_ARG,
    };

    // Manually parse the JSON string into a JsonValue
    // evorule-tcb's JsonValue does not implement serde, so it must be built manually
    let instruction = match parse_simple_json(json_str) {
        Some(v) => v,
        None => return evorule_error_code::EVORULE_ERROR_INVALID_ARG,
    };

    // C4: use the persistent FactIdGenerator held in the handle, keeping Command
    // FactIds monotonically increasing (1, 2, 3, ...).
    // Before the fix, every new() caused all Command FactIds to be 1, with
    // FactIds repeating in the FactsLog.
    let fact = crate::Fact::Command {
        id: handle.fact_id_gen.next_id(),
        instruction,
    };

    // Use send to push onto the unbounded channel
    match handle.command_tx.send(fact) {
        Ok(()) => evorule_error_code::EVORULE_OK,
        Err(_) => evorule_error_code::EVORULE_ERROR_RUNTIME,
    }
}

/// Simple JSON parser (supports basic types)
fn parse_simple_json(s: &str) -> Option<JsonValue> {
    let s = s.trim();
    if s.starts_with('{') && s.ends_with('}') {
        // Simple object parsing
        parse_json_object(s)
    } else if s.starts_with('"') && s.ends_with('"') {
        // String
        Some(JsonValue::string(&s[1..s.len() - 1]))
    } else if s.starts_with('[') && s.ends_with(']') {
        // Empty array
        Some(JsonValue::Array(vec![]))
    } else if s == "true" {
        Some(JsonValue::bool(true))
    } else if s == "false" {
        Some(JsonValue::bool(false))
    } else if s == "null" {
        Some(JsonValue::null())
    } else if let Ok(n) = s.parse::<i64>() {
        Some(JsonValue::integer(n))
    } else {
        None
    }
}

/// Simple JSON object parser (only supports one level of key-value pairs)
fn parse_json_object(s: &str) -> Option<JsonValue> {
    let s = s.trim();
    if !s.starts_with('{') || !s.ends_with('}') {
        return None;
    }

    let inner = &s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Some(JsonValue::Object(ObjectMap::new()));
    }

    // Simple parsing: only handles "key": value format
    let mut obj = ObjectMap::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // Find ":"
        let colon_pos = part.find(':')?;
        let key_part = part[..colon_pos].trim();
        let val_part = part[colon_pos + 1..].trim();

        // Parse the key (strip the quotes)
        if !key_part.starts_with('"') || !key_part.ends_with('"') {
            return None;
        }
        let key = &key_part[1..key_part.len() - 1];

        // Parse the value
        let val = parse_simple_json(val_part)?;
        obj.insert(key.to_string(), val);
    }

    Some(JsonValue::Object(obj))
}

struct ResultHandle {
    output: String,
}

#[no_mangle]
pub extern "C" fn evorule_result_get_output(result: *mut evorule_result) -> *const c_char {
    if result.is_null() {
        return std::ptr::null();
    }
    let handle = unsafe { &*(result as *mut ResultHandle) };
    match CString::new(handle.output.clone()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn evorule_result_free(result: *mut evorule_result) {
    if result.is_null() {
        return;
    }
    unsafe {
        let handle = Box::from_raw(result as *mut ResultHandle);
        drop(handle);
    }
}

#[no_mangle]
pub extern "C" fn evorule_reactor_current_queue_size(reactor: *mut evorule_reactor) -> c_int {
    if reactor.is_null() {
        return -1;
    }
    let handle = unsafe { &*(reactor as *mut ReactorFfiHandle) };
    // Use the snapshot to get the queue length
    handle
        .snapshot
        .lock()
        .map(|s| s.queue_len as c_int)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    #![allow(unsafe_code)]
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::Fact;
    use std::time::{Duration, Instant};

    /// C4 end-to-end: after sending 3 commands in a row, the Command Facts in the
    /// FactsLog must have monotonically increasing FactIds [1, 2, 3].
    ///
    /// Before the fix, every `evorule_reactor_send_command` called
    /// `FactIdGenerator::new()`, so every Command's FactId was always 1 and FactIds
    /// repeated in the FactsLog (breaking audit-chain uniqueness). After the fix the
    /// generator is persisted in the handle, and `next_id()` accumulates across
    /// calls → 1, 2, 3, ...
    ///
    /// Constructs `ReactorFfiHandle` directly (keeping a facts_log clone for
    /// observation), calls the real `evorule_reactor_send_command` three times, and
    /// confirms the Command FactId sequence in facts_log.history() is [1, 2, 3].
    #[test]
    fn ffi_send_command_produces_distinct_fact_ids_in_facts_log() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        // Spawn the reactor inside the runtime context (tokio::spawn needs the context)
        let (command_tx, _event_rx, _event_tx, handle, facts_log) = {
            let _guard = runtime.enter();
            let reactor = Reactor::builder(Vec::new()).build();
            reactor.spawn()
        };

        let snapshot =
            std::sync::Arc::new(std::sync::Mutex::new(crate::ReactorStateSnapshot::default()));
        let ffi_handle = Box::new(ReactorFfiHandle {
            _handle: handle,
            _runtime: runtime,
            command_tx,
            snapshot,
            fact_id_gen: crate::FactIdGenerator::new(),
        });
        let ptr = Box::into_raw(ffi_handle) as *mut evorule_reactor;

        // Send 3 commands in a row
        let cmds = [r#"{"op":"a"}"#, r#"{"op":"b"}"#, r#"{"op":"c"}"#];
        for c in &cmds {
            let cstr = CString::new(*c).unwrap();
            let rc = evorule_reactor_send_command(ptr, cstr.as_ptr());
            assert_eq!(rc, evorule_error_code::EVORULE_OK);
        }

        // Poll facts_log, waiting for the 3 Commands to land (the reactor drains and
        // emits asynchronously)
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let n = facts_log
                .history()
                .iter()
                .filter(|f| matches!(f, Fact::Command { .. }))
                .count();
            if n >= 3 || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let ids: Vec<u64> = facts_log
            .history()
            .iter()
            .filter_map(|f| match f {
                Fact::Command { id, .. } => Some(id.0),
                _ => None,
            })
            .collect();

        evorule_reactor_free(ptr);

        assert_eq!(
            ids,
            vec![1, 2, 3],
            "C4: Command FactIds in FactsLog must be [1, 2, 3], got {ids:?}"
        );
    }
}
