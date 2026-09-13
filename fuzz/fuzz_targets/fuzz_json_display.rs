//! Fuzz 目标：JsonValue Display 序列化
//!
//! 输入: 任意字节 → 确定性映射为深度受控（≤8）的 JsonValue → to_string()
//! 不变量: 序列化任意合法构造的 JsonValue 不得 panic、不得死循环。
//!         （字符集限定可打印 ASCII，避免 String 变体构造非法 UTF-8；
//!          递归深度由 MAX_DEPTH 封顶，杜绝栈溢出噪声。）

#![no_main]

// 经 value 模块路径导入: HEAD 与在途改动均保持 `pub mod value` 公开,
// 顶层 re-export(ObjectMap)是在途改动才有的, 顶层路径会导致 CI E0432
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
                // 0x20..=0x7E：可打印 ASCII 单字节，安全 UTF-8
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
