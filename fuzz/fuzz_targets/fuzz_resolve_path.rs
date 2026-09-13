//! Fuzz 目标：path.rs 路径解析（resolve_path / resolve_path_mut）
//!
//! 输入: 任意字节（转 UTF-8 后作为路径字符串）
//! 不变量: 解析任意路径字符串不得 panic（含多字节 UTF-8 边界、空段、
//!         超深段、非法转义、数组合法性等边界）。
//! 状态: 固定的中等工作深度 JsonValue（对象/数组/标量混合），不随输入变化。

#![no_main]

use evorule_tcb::path::{resolve_path, resolve_path_mut};
// 经 value 模块路径导入: HEAD 与在途改动均保持 `pub mod value` 公开,
// 顶层 re-export(ObjectMap)是在途改动才有的, 顶层路径会导致 CI E0432
use evorule_tcb::value::{JsonValue, ObjectMap};
use libfuzzer_sys::fuzz_target;
use std::str::from_utf8;

/// 构造固定测试状态:
/// { "a": {"x": 42, "name": "evorule"},
///   "list": [1, "two", true],
///   "deep": {"l2": {"l3": [{"tail": "ok"}]}} }
fn fixed_state() -> JsonValue {
    let mut l3_obj = ObjectMap::new();
    l3_obj.insert("tail".to_string(), JsonValue::from("ok"));

    let mut deep_l3 = Vec::new();
    deep_l3.push(JsonValue::from(l3_obj));

    let mut l2 = ObjectMap::new();
    l2.insert("l3".to_string(), JsonValue::from(deep_l3));

    let mut deep = ObjectMap::new();
    deep.insert("l2".to_string(), JsonValue::from(l2));

    let mut inner = ObjectMap::new();
    inner.insert("x".to_string(), JsonValue::from(42i64));
    inner.insert("name".to_string(), JsonValue::from("evorule"));

    let mut list = Vec::new();
    list.push(JsonValue::from(1i64));
    list.push(JsonValue::from("two"));
    list.push(JsonValue::from(true));

    let mut root = ObjectMap::new();
    root.insert("a".to_string(), JsonValue::from(inner));
    root.insert("list".to_string(), JsonValue::from(list));
    root.insert("deep".to_string(), JsonValue::from(deep));

    JsonValue::from(root)
}

fuzz_target!(|data: &[u8]| {
    if let Ok(path) = from_utf8(data) {
        // 只读解析
        let state = fixed_state();
        let _ = resolve_path(&state, path);

        // 可变解析（作用于独立克隆状态，结果丢弃）
        let mut state_mut = fixed_state();
        let _ = resolve_path_mut(&mut state_mut, path);
    }
});
