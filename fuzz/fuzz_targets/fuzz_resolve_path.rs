//! Fuzz target: path.rs path resolution (resolve_path / resolve_path_mut)
//!
//! Input: arbitrary bytes (converted to UTF-8 and used as the path string)
//! Invariant: resolving any path string must not panic (covering multi-byte
//!         UTF-8 boundaries, empty segments, overly deep segments, invalid
//!         escapes, array validity, and other edges).
//! State: a fixed medium-depth JsonValue (objects/arrays/scalars mixed); it
//!        does not vary with the input.

#![no_main]

use evorule_tcb::path::{resolve_path, resolve_path_mut};
// Imported via the value module path: both HEAD and in-flight changes keep
// `pub mod value` public; the top-level re-export (ObjectMap) only exists in
// in-flight changes, and the top-level path would cause CI E0432
use evorule_tcb::value::{JsonValue, ObjectMap};
use libfuzzer_sys::fuzz_target;
use std::str::from_utf8;

/// Build the fixed test state:
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
        // Read-only resolution
        let state = fixed_state();
        let _ = resolve_path(&state, path);

        // Mutable resolution (applied to an independent cloned state; the result is discarded)
        let mut state_mut = fixed_state();
        let _ = resolve_path_mut(&mut state_mut, path);
    }
});
