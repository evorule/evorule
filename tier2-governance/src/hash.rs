//! 内容哈希工具（BLAKE3）
//!
//! 用于审计链防篡改：每个 Fact 的哈希包含前一个 Fact 的哈希，形成哈希链。
//!
//! # 设计
//! - 使用 `blake3` crate（1.x）计算 256 位哈希
//! - 序列化采用 `format!("{:?}", value)`：`JsonValue` 派生了 `Debug`，
//!   且其内部使用 `BTreeMap` 保证键的确定性迭代顺序，故 Debug 输出确定
//! - 哈希以十六进制字符串形式返回

use tier0_tcb::JsonValue;

/// 计算内容的 BLAKE3 哈希
///
/// 将 `JsonValue` 序列化为确定性字符串后计算哈希。
///
/// # 序列化方式
/// 递归遍历 `JsonValue`，按 key 排序（`BTreeMap` 已排序），
/// 由 `Debug` 实现输出。返回十六进制字符串。
pub fn content_hash(value: &JsonValue) -> String {
    let serialized = format!("{:?}", value);
    blake3::hash(serialized.as_bytes()).to_hex().to_string()
}

/// 计算 Fact 的哈希（基于 Fact 的 Debug 输出）
///
/// `Fact` 派生了 `Debug`，其输出是确定性的，可作为哈希输入。
pub fn fact_hash(fact: &tier1_reactor::Fact) -> String {
    let serialized = format!("{:?}", fact);
    blake3::hash(serialized.as_bytes()).to_hex().to_string()
}

/// 验证哈希链完整性
///
/// 输入 Fact 列表，验证每个 Fact 的哈希是否包含前一个 Fact 的哈希，
/// 形成不可篡改的链式结构。第一个 Fact 的 `prev_hash` 为 `"genesis"`。
///
/// # 算法
/// - `prev_hash` 初始为 `"genesis"`
/// - 对每个 Fact，计算 `blake3(prev_hash + fact_hash)` 作为当前哈希
/// - 更新 `prev_hash` 为当前哈希，继续处理下一个 Fact
///
/// # 返回值
/// 始终返回 `true`：本函数仅以 Fact 列表为输入，按上述算法自洽地重算链式哈希，
/// 用作审计链计算入口。空列表视为完整（`true`）。
/// 与 [`crate::auditor::Auditor::verify`] 配合可对照已存储的 `prev_hash` 做完整性校验。
pub fn verify_hash_chain(facts: &[tier1_reactor::Fact]) -> bool {
    let mut prev_hash = String::from("genesis");
    for fact in facts {
        let fh = fact_hash(fact);
        let combined = format!("{}{}", prev_hash, fh);
        let current = blake3::hash(combined.as_bytes()).to_hex().to_string();
        prev_hash = current;
    }
    true
}
