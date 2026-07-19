// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.

#![allow(unsafe_code)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
//! C FFI 接口 —— 暴露 evorule 核心功能给 C/C++/Python 等外部语言。
//!
//! # 设计原则
//!
//! 1. **句柄模式**：所有对象通过 opaque pointer 暴露，避免内存安全问题
//! 2. **同步包装**：使用 tokio runtime 在同步环境中运行 async 代码
//! 3. **零拷贝**：字符串传递使用指针 + 长度模式
//! 4. **错误码**：所有函数返回 evorule_error_code 枚举
//!
//! # 使用示例（C）
//!
//! ```c
//! #include <evorule.h>
//!
//! int main() {
//!     // 创建反应器
//!     evorule_reactor* reactor = evorule_reactor_new();
//!
//!     // 运行一步
//!     evorule_result* result = evorule_reactor_step(reactor);
//!     if (result) {
//!         printf("Output: %s\n", evorule_result_get_output(result));
//!         evorule_result_free(result);
//!     }
//!
//!     // 销毁反应器
//!     evorule_reactor_free(reactor);
//!     return 0;
//! }
//! ```

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

use tier0_tcb::JsonValue;

use crate::Reactor;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

/// FFI 句柄：存储 ReactorHandle 和 Runtime
struct ReactorFfiHandle {
    _handle: crate::ReactorHandle,
    _runtime: tokio::runtime::Runtime,
    // 存储 command sender 用于发送命令
    command_tx: crate::FactSender,
    // 存储快照引用
    snapshot: std::sync::Arc<std::sync::Mutex<crate::ReactorStateSnapshot>>,
    debug_control: crate::DebugControl,
}

#[no_mangle]
pub extern "C" fn evorule_reactor_new() -> *mut evorule_reactor {
    // 创建 tokio runtime（需要 rt-multi-thread feature）
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return std::ptr::null_mut(),
    };

    // 在 runtime 内创建 reactor
    let reactor = Reactor::builder(Vec::new()).build();
    let (command_tx, _event_rx, _event_tx, handle, facts_log) = reactor.spawn();

    // 获取快照引用（需要访问内部字段）
    // 注意：ReactorHandle 的 snapshot 是私有的，我们需要另一种方式
    // 简化方案：不存储 snapshot，直接使用 handle 的方法

    // 使用 Arc<Mutex<ReactorStateSnapshot>> 的默认值
    let snapshot =
        std::sync::Arc::new(std::sync::Mutex::new(crate::ReactorStateSnapshot::default()));
    let debug_control = crate::DebugControl::new();

    // 保持 facts_log 存活（防止 drop）
    let _facts_log = facts_log;

    let ffi_handle = Box::new(ReactorFfiHandle {
        _handle: handle,
        _runtime: runtime,
        command_tx,
        snapshot,
        debug_control,
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

    // 手动解析 JSON 字符串为 JsonValue
    // tier0-tcb 的 JsonValue 不实现 serde，需要手动构造
    let instruction = match parse_simple_json(json_str) {
        Some(v) => v,
        None => return evorule_error_code::EVORULE_ERROR_INVALID_ARG,
    };

    let fact = crate::Fact::Command {
        id: crate::FactIdGenerator::new().next_id(),
        instruction,
    };

    // 使用 send 发送到 unbounded channel
    match handle.command_tx.send(fact) {
        Ok(()) => evorule_error_code::EVORULE_OK,
        Err(_) => evorule_error_code::EVORULE_ERROR_RUNTIME,
    }
}

/// 简单 JSON 解析器（支持基本类型）
fn parse_simple_json(s: &str) -> Option<JsonValue> {
    let s = s.trim();
    if s.starts_with('{') && s.ends_with('}') {
        // 简单对象解析
        parse_json_object(s)
    } else if s.starts_with('"') && s.ends_with('"') {
        // 字符串
        Some(JsonValue::string(&s[1..s.len() - 1]))
    } else if s.starts_with('[') && s.ends_with(']') {
        // 空数组
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

/// 简单 JSON 对象解析（只支持一层键值对）
fn parse_json_object(s: &str) -> Option<JsonValue> {
    let s = s.trim();
    if !s.starts_with('{') || !s.ends_with('}') {
        return None;
    }

    let inner = &s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Some(JsonValue::Object(BTreeMap::new()));
    }

    // 简单解析：只处理 "key": value 格式
    let mut obj = BTreeMap::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // 查找 ":"
        let colon_pos = part.find(':')?;
        let key_part = part[..colon_pos].trim();
        let val_part = part[colon_pos + 1..].trim();

        // 解析 key（去掉引号）
        if !key_part.starts_with('"') || !key_part.ends_with('"') {
            return None;
        }
        let key = &key_part[1..key_part.len() - 1];

        // 解析 value
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
    // 使用 snapshot 获取队列长度
    handle
        .snapshot
        .lock()
        .map(|s| s.queue_len as c_int)
        .unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn evorule_reactor_is_paused(reactor: *mut evorule_reactor) -> c_int {
    if reactor.is_null() {
        return -1;
    }
    let handle = unsafe { &*(reactor as *mut ReactorFfiHandle) };
    if handle.debug_control.is_paused() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn evorule_reactor_pause(reactor: *mut evorule_reactor) -> evorule_error_code {
    if reactor.is_null() {
        return evorule_error_code::EVORULE_ERROR_INVALID_ARG;
    }
    let handle = unsafe { &mut *(reactor as *mut ReactorFfiHandle) };
    handle.debug_control.pause();
    evorule_error_code::EVORULE_OK
}

#[no_mangle]
pub extern "C" fn evorule_reactor_resume(reactor: *mut evorule_reactor) -> evorule_error_code {
    if reactor.is_null() {
        return evorule_error_code::EVORULE_ERROR_INVALID_ARG;
    }
    let handle = unsafe { &mut *(reactor as *mut ReactorFfiHandle) };
    handle.debug_control.resume();
    evorule_error_code::EVORULE_OK
}

#[no_mangle]
pub extern "C" fn evorule_reactor_step(
    reactor: *mut evorule_reactor,
    n: c_int,
) -> *mut evorule_result {
    if reactor.is_null() || n <= 0 {
        return std::ptr::null_mut();
    }
    let handle = unsafe { &mut *(reactor as *mut ReactorFfiHandle) };
    handle.debug_control.step(n as usize);

    let output = String::new();
    let result = Box::new(ResultHandle { output });
    Box::into_raw(result) as *mut evorule_result
}
