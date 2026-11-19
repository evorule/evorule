//! Fuzz target: JsonValue Display serialization
//!
//! Input: arbitrary bytes → deterministically map to a JsonValue with bounded
//! depth (≤8) → to_string()
//! Invariant: serializing any legally constructed JsonValue must not panic
//! and must not loop forever.
//!         (The charset is restricted to printable ASCII, avoiding invalid
//!          UTF-8 in the String variant; recursion depth is capped by
//!          MAX_DEPTH, eliminating stack-overflow noise.)

#![no_main]

// Imported via the value module path: both HEAD and in-flight changes keep
// `pub mod value` public; the top-level re-export (ObjectMap) only exists in
// in-flight changes, and the top-level path would cause CI E0432
use evorule_tcb::value::{JsonValue, ObjectMap};
use libfuzzer_sys::fuzz_target;

const MAX_DEPTH: u32 = 8;
const MAX_STR: usize = 256;

fn byte_at(data: &[u8], pos: &mut usize) -> u8 {
    if *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        b
    } else {
        0
    }
}

fn build_value(data: &[u8], depth: u32, pos: &mut usize) -> JsonValue {
    let tag = byte_at(data, pos);
    match tag % 6 {
        0 => JsonValue::Null,
        1 => JsonValue::Bool(byte_at(data, pos) & 1 == 1),
        2 => {
            let mut v: i64 = 0;
            for _ in 0..8 {
                v = (v << 8) | byte_at(data, pos) as i64;
            }
            JsonValue::Integer(v)
        }
        3 => {
            let n = (byte_at(data, pos) as usize) % (MAX_STR + 1);
            let mut s = String::with_capacity(n);
            for _ in 0..n {
                // 0x20..=0x7E: printable single-byte ASCII, safe UTF-8
                let b = 0x20 + (byte_at(data, pos) % 0x5F);
                s.push(b as char);
            }
            JsonValue::from(s)
        }
        4 => {
            let mut items = Vec::new();
            if depth > 0 {
                let n = (byte_at(data, pos) as usize) % 6;
                for _ in 0..n {
                    items.push(build_value(data, depth - 1, pos));
                }
            }
            JsonValue::from(items)
        }
        _ => {
            let mut obj = ObjectMap::new();
            if depth > 0 {
                let n = (byte_at(data, pos) as usize) % 6;
                for i in 0..n {
                    let key = format!("k{pos}_{i}");
                    obj.insert(key, build_value(data, depth - 1, pos));
                }
            }
            JsonValue::from(obj)
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut pos = 0usize;
    let value = build_value(data, MAX_DEPTH, &mut pos);
    let _serialized = value.to_string();
});
