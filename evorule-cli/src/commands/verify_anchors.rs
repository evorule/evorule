// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule verify-anchors` —— 离线校验 G-A1 审计锚点真实性（防抵赖）
//!
//! 输入为 `evorule-governance` `Auditor::export()` 产生的审计导出 JSON
//!（含 `verifying_key` + `anchors` 数组）。本命令逐条校验：
//! 1. **锚点链式链接**：每个锚点 `prev_anchor_hash` 必须等于上一锚点的自哈希（首锚为 `genesis`），
//!    防止中间锚点被截断/丢弃。
//! 2. **签名真实性**：用公钥重算每个锚点的规范化载荷并验签，证明审计链确由私钥持有者生成
//!    （非"仅检篡改"，而是"可证来源/防抵赖"）。
//!
//! 未配置私钥/公钥亦可验证——私钥只在生成方，公钥随导出物分发，本命令仅需公钥。

use std::path::Path;

use serde_json::Value;

use crate::error::CliError;
use crate::signing;

/// 从 64 位 hex 还原 32 字节公钥；非法则返回错误定位
fn parse_pubkey(hex: &str) -> Result<[u8; 32], CliError> {
    let bytes = signing::hex_decode(hex).map_err(|e| CliError::other(e.to_string()))?;
    if bytes.len() != 32 {
        return Err(CliError::other(format!("公钥长度 != 32: {}", bytes.len())));
    }
    let mut arr = [0u8; 32];
    for (dst, src) in arr.iter_mut().zip(bytes.iter()) {
        *dst = *src;
    }
    Ok(arr)
}

/// 从 128 位 hex 还原 64 字节签名；非法则返回错误定位
fn parse_signature(hex: &str) -> Result<[u8; 64], CliError> {
    let bytes = signing::hex_decode(hex).map_err(|e| CliError::other(e.to_string()))?;
    if bytes.len() != 64 {
        return Err(CliError::other(format!("签名长度 != 64: {}", bytes.len())));
    }
    let mut arr = [0u8; 64];
    for (dst, src) in arr.iter_mut().zip(bytes.iter()) {
        *dst = *src;
    }
    Ok(arr)
}

/// 重算锚点的规范化载荷（cli 侧锚点规范的定义处）
///
/// 字段固定为 seq/version/entry_count/last_hash/prev_anchor_hash，经 `serde_json::json!` 按
/// BTree 字典序序列化（无 preserve_order）→ 确定性载荷，验签时同法重算。
fn anchor_payload(
    seq: u64,
    version: u64,
    entry_count: usize,
    last_hash: &str,
    prev_anchor_hash: &str,
) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "seq": seq,
        "version": version,
        "entry_count": entry_count,
        "last_hash": last_hash,
        "prev_anchor_hash": prev_anchor_hash,
    }))
    .unwrap_or_default()
}

/// 锚点自哈希 = blake3(载荷 + 签名)，供下一锚点链接
fn anchor_self_hash(payload: &[u8], signature: &[u8; 64]) -> String {
    let mut preimage = payload.to_vec();
    preimage.extend_from_slice(signature);
    blake3::hash(&preimage).to_hex().to_string()
}

/// 校验单个锚点字段并返回载荷/签名，缺失或非法则返回定位错误
fn parse_anchor(value: &Value) -> Result<(Vec<u8>, [u8; 64]), CliError> {
    let seq = value
        .get("seq")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("锚点缺 seq"))?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("锚点缺 version"))?;
    let entry_count = value
        .get("entry_count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("锚点缺 entry_count"))? as usize;
    let last_hash = value
        .get("last_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::other("锚点缺 last_hash"))?
        .to_string();
    let prev_anchor_hash = value
        .get("prev_anchor_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::other("锚点缺 prev_anchor_hash"))?
        .to_string();
    let sig_hex = value
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::other("锚点缺 signature"))?;
    let signature = parse_signature(sig_hex)?;
    let payload = anchor_payload(seq, version, entry_count, &last_hash, &prev_anchor_hash);
    Ok((payload, signature))
}

/// 执行 verify-anchors 子命令
///
/// # 参数
/// - `audit_path`：审计导出 JSON 文件路径
/// - `pubkey_hex`：可选公钥 hex；缺省时使用导出物内嵌的 `verifying_key`
///
/// # 退出码
/// - 0：全部锚点签名有效且链式链接完整
/// - 1：任一锚点被篡改/删改/错签或输入非法
pub fn run(audit_path: &Path, pubkey_hex: Option<&str>) -> Result<(), CliError> {
    let json_str = std::fs::read_to_string(audit_path)
        .map_err(|e| CliError::Other(format!("读取 {} 失败: {e}", audit_path.display())))?;
    let parsed: Value =
        serde_json::from_str(&json_str).map_err(|e| CliError::other(format!("JSON 解析失败: {e}")))?;

    // 1. 解析公钥（优先命令行，其次导出物内嵌 verifying_key）
    let pk_bytes = if let Some(hex) = pubkey_hex {
        parse_pubkey(hex)?
    } else {
        let embedded = parsed
            .get("verifying_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CliError::other("导出物未含 verifying_key，且未提供 --pubkey，无法验证真实性")
            })?;
        parse_pubkey(embedded)?
    };

    // 2. 读取锚点数组
    let anchors_val = parsed
        .get("anchors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CliError::other("导出物缺 anchors 数组"))?;

    if anchors_val.is_empty() {
        println!("=== Verify Anchors: {} ===", audit_path.display());
        println!("[WARN] 无审计锚点（未配置签名器则无真实性证据，仅哈希链完整性）");
        return Ok(());
    }

    println!("=== Verify Anchors: {} ===", audit_path.display());
    println!("Algorithm: ed25519 (RFC 8032) deterministic signature");
    println!("Public key: {}", signing::hex_encode(&pk_bytes));
    println!();

    let verified = verify_value(&parsed, pk_bytes)?;

    println!();
    println!("[OK] 全部 {} 个锚点签名有效且链式链接完整", verified);
    Ok(())
}

/// 核心校验逻辑（独立成函数以便单元测试）：逐锚点校验链式链接 + 签名真实性
///
/// 返回校验通过的锚点数量；任一失败返回 `Err` 精确报错定位。成功输出逐锚点结果。
fn verify_value(parsed: &Value, pk_bytes: [u8; 32]) -> Result<usize, CliError> {
    let anchors_val = parsed
        .get("anchors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CliError::other("导出物缺 anchors 数组"))?;

    let mut prev_anchor_hash = String::from("genesis");
    for (i, anchor_val) in anchors_val.iter().enumerate() {
        let (payload, signature) = parse_anchor(anchor_val)?;
        let seq = i as u64 + 1;

        // 2a. 锚点链式链接校验
        let stored_prev = anchor_val
            .get("prev_anchor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if stored_prev != prev_anchor_hash {
            return Err(CliError::HashChain(format!(
                "锚点链断裂 @seq={}: 期望 prev_anchor_hash={} 实得={}",
                seq, prev_anchor_hash, stored_prev
            )));
        }

        // 2b. 签名真实性校验
        let ok = signing::verify_signature(pk_bytes, &payload, &signature)
            .map_err(|e| CliError::other(e.to_string()))?;
        if !ok {
            return Err(CliError::HashChain(format!(
                "锚点 @seq={} 签名校验失败（数据被篡改或非本公钥签名）",
                seq
            )));
        }

        prev_anchor_hash = anchor_self_hash(&payload, &signature);
        println!(
            "[OK] anchor#{} seq={} entry_count={} last_hash={}",
            i + 1, seq, anchor_val["entry_count"], anchor_val["last_hash"]
        );
    }

    Ok(anchors_val.len())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// 构造一份"导出物 JSON"（含 verifying_key + 一串已签名的链式锚点）
    fn build_export_json(seed: u8, count: usize) -> (Value, [u8; 32]) {
        let signer = crate::signing::AuditSigner::from_bytes([seed; 32]);
        let pk = signer.verifying_bytes();
        let mut anchors = Vec::new();
        let mut chain_prev = String::from("genesis");
        for idx in 1..=count as u64 {
            let last_hash = format!("last-hash-{}", idx);
            let payload =
                anchor_payload(idx, idx * 10, (idx as usize) * 5, &last_hash, &chain_prev);
            let sig = signer.signature_bytes(&payload);
            anchors.push(serde_json::json!({
                "seq": idx,
                "version": idx * 10,
                "entry_count": (idx as usize) * 5,
                "last_hash": last_hash,
                "prev_anchor_hash": chain_prev.clone(),
                "signature": crate::signing::hex_encode(&sig),
            }));
            chain_prev = anchor_self_hash(&payload, &sig);
        }
        let json = serde_json::json!({
            "version": "1.0",
            "verifying_key": crate::signing::hex_encode(&pk),
            "anchors": anchors,
        });
        (json, pk)
    }

    #[test]
    fn test_verify_value_ok() {
        let (json, pk) = build_export_json(7, 3);
        assert_eq!(verify_value(&json, pk).unwrap(), 3);
    }

    #[test]
    fn test_verify_value_detects_tampered_last_hash() {
        let (mut json, pk) = build_export_json(7, 2);
        json["anchors"][0]["last_hash"] = serde_json::json!("tampered");
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("签名校验失败"));
    }

    #[test]
    fn test_verify_value_detects_severed_chain() {
        let (mut json, pk) = build_export_json(9, 3);
        // 删除中间锚点 → 第 3 个的 prev_anchor_hash 不再等于第 1 个的自哈希
        json["anchors"].as_array_mut().unwrap().remove(1);
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("锚点链断裂"));
    }

    #[test]
    fn test_verify_value_detects_wrong_pubkey() {
        let (json, _) = build_export_json(7, 2);
        // 用另一把私钥的公钥验证 → 全部失败
        let wrong = crate::signing::AuditSigner::from_bytes([99u8; 32]).verifying_bytes();
        let err = verify_value(&json, wrong).unwrap_err();
        assert!(err.to_string().contains("签名校验失败"));
    }

    #[test]
    fn test_verify_value_missing_anchors() {
        let json = serde_json::json!({ "version": "1.0" });
        let err = verify_value(&json, [0u8; 32]).unwrap_err();
        assert!(err.to_string().contains("缺 anchors"));
    }
}