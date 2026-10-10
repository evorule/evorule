// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule verify-anchors` —— 离线校验审计锚点链（防抵赖）
//!
//! 输入为 evorule-server `GET /api/sessions/{id}/anchors` 产物
//! （`{session_id, count, anchors: [...]}`，或裸 `anchors` 数组）。
//! 逐锚点校验（evorule-anchor/2 格式，governance ≥0.8.2）：
//! 1. **seq 连续**：0 起严格 +1；
//! 2. **区间无缝**：首锚 lo=0，相邻锚点 prev.hi == next.lo；
//! 3. **签名真实性**：按 v2 载荷串接（`signature_payload_v2` 同构）重算并
//!    用公钥验签——证明锚点确由私钥持有者生成（防抵赖），且篡改任何
//!    载荷字段即失败。
//!
//! 私钥只在生成方；公钥随锚点物分发，本命令仅需公钥。
//! blake3 锚点哈希链重算（A2 截断/fork 检出）由 evorule verify 端点背书
//! （诚实边界：与 evo-agent tools/verify_anchors.py 同口径）。

use std::path::Path;

use serde_json::Value;

use crate::error::CliError;
use crate::signing;

/// v2 载荷串接（与 evorule-governance `signature_payload_v2` 同构；
/// 字段序=结构体声明序，`|` 分隔）
fn anchor_payload_v2(anchor: &Value) -> Result<String, CliError> {
    let get_str = |key: &str| -> Result<String, CliError> {
        anchor
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| CliError::other(format!("Anchor missing '{key}'")))
    };
    let get_u64 = |key: &str| -> Result<u64, CliError> {
        anchor
            .get(key)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other(format!("Anchor missing '{key}'")))
    };
    let lo = anchor
        .pointer("/fact_range/lo")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("Anchor missing 'fact_range.lo'"))?;
    let hi = anchor
        .pointer("/fact_range/hi")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("Anchor missing 'fact_range.hi'"))?;
    Ok(format!(
        "evorule-anchor/2|{}|{}|{}|{}|{}|{}|{}|{}",
        get_str("session_id")?,
        get_u64("seq")?,
        lo,
        hi,
        get_str("chain_head")?,
        get_str("key_id")?,
        get_str("engine_id")?,
        get_u64("logical_time")?,
    ))
}

/// 从 64 位 hex 还原 32 字节公钥；非法则返回错误定位
fn parse_pubkey(hex: &str) -> Result<[u8; 32], CliError> {
    let bytes = signing::hex_decode(hex).map_err(|e| CliError::other(e.to_string()))?;
    if bytes.len() != 32 {
        return Err(CliError::other(format!(
            "Public key length != 32: {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// 执行 verify-anchors 子命令
///
/// # 参数
/// - `anchors_path`：锚点列表 JSON（server `/anchors` 产物或裸数组）
/// - `pubkey_hex`：可选公钥 hex；缺省使用文档内嵌 `verifying_key`
///
/// # 退出码
/// - 0：全部锚点结构+签名有效
/// - 1：任一锚点被篡改/断裂/错签或输入非法
pub fn run(anchors_path: &Path, pubkey_hex: Option<&str>) -> Result<(), CliError> {
    let json_str = std::fs::read_to_string(anchors_path)
        .map_err(|e| CliError::Other(format!("Read {} failed: {e}", anchors_path.display())))?;
    let parsed: Value = serde_json::from_str(&json_str)
        .map_err(|e| CliError::other(format!("JSON parse failed: {e}")))?;

    // 公钥：命令行 > 文档内嵌 verifying_key
    let pk_hex = if let Some(hex) = pubkey_hex {
        hex.to_string()
    } else {
        parsed
            .get("verifying_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CliError::other("No --pubkey provided and document has no embedded 'verifying_key'")
            })?
            .to_string()
    };
    let pk = parse_pubkey(&pk_hex)?;

    println!("=== Verify Anchors (evorule-anchor/2): {} ===", anchors_path.display());
    println!("Algorithm: ed25519 (RFC 8032) over v2 payload concatenation");
    println!("Public key: {pk_hex}");
    println!();

    let verified = verify_value(&parsed, pk)?;

    println!();
    println!(
        "[OK] All {verified} anchors verified: seq contiguous, ranges seamless, signatures valid"
    );
    Ok(())
}

/// 核心校验（独立成函数以便单元测试）：结构三查 + 逐锚验签
fn verify_value(parsed: &Value, pk: [u8; 32]) -> Result<usize, CliError> {
    let anchors_val = parsed
        .get("anchors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CliError::other("Document missing 'anchors' array"))?;

    if anchors_val.is_empty() {
        println!("[WARN] No anchors (anchor chain not enabled for this session)");
        return Ok(0);
    }

    let mut expect_lo: u64 = 0;
    for (i, a) in anchors_val.iter().enumerate() {
        // 1. seq 连续（0 起严格 +1）
        let seq = a
            .get("seq")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'seq'"))?;
        if seq != i as u64 {
            return Err(CliError::other(format!(
                "Anchor [{i}] seq broken: expected {i}, got {seq}"
            )));
        }
        // 2. 区间无缝
        let lo = a
            .pointer("/fact_range/lo")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'fact_range.lo'"))?;
        let hi = a
            .pointer("/fact_range/hi")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'fact_range.hi'"))?;
        if lo != expect_lo {
            return Err(CliError::other(format!(
                "Anchor [{i}] fact range not seamless: expected lo={expect_lo}, got lo={lo}"
            )));
        }
        expect_lo = hi;

        // 3. v2 载荷验签
        let payload = anchor_payload_v2(a)?;
        let sig_hex = a
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::other("Anchor missing 'signature'"))?;
        let sig_bytes = signing::hex_decode(sig_hex)
            .map_err(|e| CliError::other(e.to_string()))?;
        let mut sig_arr = [0u8; 64];
        if sig_bytes.len() != 64 {
            return Err(CliError::other(format!(
                "Anchor [{i}] signature length != 64: {}",
                sig_bytes.len()
            )));
        }
        sig_arr.copy_from_slice(&sig_bytes);
        let ok = signing::verify_signature(pk, payload.as_bytes(), &sig_arr)
            .map_err(|e| CliError::other(e.to_string()))?;
        if !ok {
            return Err(CliError::other(format!(
                "Anchor [{i}] signature verification failed (tampered payload or wrong key)"
            )));
        }
        println!(
            "[{i}] seq={seq} range=[{lo},{hi}) head={}… signature: OK",
            &a.get("chain_head")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .chars()
                .take(12)
                .collect::<String>()
        );
    }
    Ok(anchors_val.len())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// 构造 v2 锚点链（与 governance signature_payload_v2 同源签名）
    fn build_anchors(seed: u8, count: usize) -> (Value, [u8; 32]) {
        let signer = crate::signing::AuditSigner::from_bytes([seed; 32]);
        let pk = signer.verifying_bytes();
        let mut anchors = Vec::new();
        let mut lo = 0u64;
        for i in 0..count {
            let hi = lo + 5;
            let payload = format!(
                "evorule-anchor/2|sess-1|{i}|{lo}|{hi}|{}|anchor-key-1|vec-engine/1|0",
                "ab".repeat(32)
            );
            let sig = signer.signature_bytes(payload.as_bytes());
            anchors.push(serde_json::json!({
                "anchor_format": "evorule-anchor/2",
                "seq": i,
                "session_id": "sess-1",
                "fact_range": {"lo": lo, "hi": hi},
                "chain_head": "ab".repeat(32),
                "key_id": "anchor-key-1",
                "engine_id": "vec-engine/1",
                "logical_time": 0,
                "anchor_hash": "00".repeat(32),
                "signature": crate::signing::hex_encode(&sig),
            }));
            lo = hi;
        }
        (
            serde_json::json!({
                "session_id": "sess-1",
                "count": count,
                "verifying_key": crate::signing::hex_encode(&pk),
                "anchors": anchors,
            }),
            pk,
        )
    }

    #[test]
    fn test_v2_chain_ok() {
        let (json, pk) = build_anchors(7, 3);
        assert_eq!(verify_value(&json, pk).unwrap(), 3);
    }

    #[test]
    fn test_v2_detects_tampered_payload() {
        let (mut json, pk) = build_anchors(7, 2);
        json["anchors"][0]["chain_head"] = serde_json::json!("ff".to_string() + &"ab".repeat(31));
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("signature verification failed"));
    }

    #[test]
    fn test_v2_detects_seq_break() {
        let (mut json, pk) = build_anchors(9, 3);
        json["anchors"].as_array_mut().unwrap().remove(1);
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("seq broken"));
    }

    #[test]
    fn test_v2_detects_range_gap() {
        let (mut json, pk) = build_anchors(9, 2);
        json["anchors"][1]["fact_range"]["lo"] = serde_json::json!(99);
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("not seamless"));
    }

    #[test]
    fn test_v2_detects_wrong_pubkey() {
        let (json, _) = build_anchors(7, 2);
        let wrong = crate::signing::AuditSigner::from_bytes([99u8; 32]).verifying_bytes();
        let err = verify_value(&json, wrong).unwrap_err();
        assert!(err.to_string().contains("signature verification failed"));
    }

    #[test]
    fn test_v2_empty_anchors() {
        let json = serde_json::json!({"count": 0, "anchors": []});
        assert_eq!(verify_value(&json, [0u8; 32]).unwrap(), 0);
    }
}
