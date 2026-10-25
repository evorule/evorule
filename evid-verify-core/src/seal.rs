// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 锚点封条验签（v2 载荷语义，公钥注入式）
//!
//! 与 server 侧 `patches/evorule-governance/src/anchor.rs` 逐字节对齐：
//! - 签名目标 = [`signature_payload_v2`] 载荷串接字节
//!   （`evorule-anchor/2`，0.8.2 起）；v1 锚点（0.8.1，签名目标 =
//!   anchor_hash hex）不兼容，混版链判失败（诚实边界：不静默跨格式验过）。
//! - `anchor_hash = blake3(prev_anchor_hash | 载荷九字段)`，字段以 `|`
//!   串接，链首 prev = [`ANCHOR_GENESIS`]。
//!
//! 本模块不含密钥生成/时钟/随机——公钥由调用方注入（`key_id` 查表），
//! TCB 纪律不破。跨仓一致性由锚点向量闸守护（S4 CI 移动端闸批次）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};

/// 锚点签名载荷格式版本（v2：字段串接字节，零依赖可验）
pub const ANCHOR_FORMAT: &str = "evorule-anchor/2";

/// 锚点链哈希算法的创世值（与事实链 genesis 语义对齐，独立命名空间）
pub const ANCHOR_GENESIS: &str = "anchor-genesis";

/// 被审计的事实序号区间（前闭后开）：`[lo, hi)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactRange {
    pub lo: u64,
    pub hi: u64,
}

/// 审计锚点封条（字段名与 server 侧 `AuditAnchor` serde 同形，可直接反序列化）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorSeal {
    /// 锚点链序号（0 起，严格 +1）
    pub seq: u64,
    /// 会话标识
    pub session_id: String,
    /// 覆盖的事实序号区间 [lo, hi)
    pub fact_range: FactRange,
    /// 截至本锚点的事实链链头哈希
    pub chain_head: String,
    /// 签名密钥标识（验证方按 key_id 选公钥）
    pub key_id: String,
    /// 引擎/部署标识
    pub engine_id: String,
    /// 逻辑时刻（LogicalClock 供给，非墙钟）
    pub logical_time: u64,
    /// 载荷哈希 = blake3(prev_anchor_hash | 载荷字段)
    pub anchor_hash: String,
    /// ed25519 签名（64 字节的 hex，128 字符）
    pub signature: String,
}

/// v2 签名载荷：字段以 `|` 串接（与 anchor_hash 同字段序，不含 prev——
/// 签名只锚本锚点自身内容，链式关系由 anchor_hash 链承载）
pub fn signature_payload_v2(seal: &AnchorSeal) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ANCHOR_FORMAT,
        seal.session_id,
        seal.seq,
        seal.fact_range.lo,
        seal.fact_range.hi,
        seal.chain_head,
        seal.key_id,
        seal.engine_id,
        seal.logical_time,
    )
}

/// 计算锚点载荷哈希（`anchor_hash = blake3(prev | 载荷九字段)`）
pub fn compute_anchor_hash(prev_anchor_hash: &str, seal: &AnchorSeal) -> String {
    let combined = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        prev_anchor_hash,
        seal.session_id,
        seal.seq,
        seal.fact_range.lo,
        seal.fact_range.hi,
        seal.chain_head,
        seal.key_id,
        seal.engine_id,
        seal.logical_time,
    );
    blake3::hash(combined.as_bytes()).to_hex().to_string()
}

/// 锚点验签错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// 载荷哈希不匹配（字段被篡改或 prev 链接错误）
    PayloadHashMismatch { seq: u64 },
    /// 签名验证失败（公钥不匹配/签名伪造）
    SignatureInvalid { seq: u64 },
    /// 锚点链结构错误（seq 跳跃/区间断裂/倒挂）
    ChainBroken { seq: u64, reason: String },
    /// key_id 无对应公钥
    KeyMissing { key_id: String },
    /// 公钥/签名 hex 非法
    HexInvalid { seq: u64, reason: String },
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SealError::PayloadHashMismatch { seq } => {
                write!(
                    f,
                    "锚点[{seq}] 载荷哈希不匹配（字段被篡改或 prev 链接错误）"
                )
            }
            SealError::SignatureInvalid { seq } => write!(f, "锚点[{seq}] 签名验证失败"),
            SealError::ChainBroken { seq, reason } => {
                write!(f, "锚点链断裂于[{seq}]: {reason}")
            }
            SealError::KeyMissing { key_id } => write!(f, "密钥标识 \"{key_id}\" 无对应公钥"),
            SealError::HexInvalid { seq, reason } => write!(f, "锚点[{seq}] hex 非法: {reason}"),
        }
    }
}

impl core::error::Error for SealError {}

/// hex 解码（偶长 + 16 进制字符校验）
pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// hex 编码（测试正例构造用）
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// 验证单个锚点：重算载荷哈希 + 验签名（v2 载荷字节）
pub fn verify_seal(
    seal: &AnchorSeal,
    prev_anchor_hash: &str,
    verifying_key_hex: &str,
) -> Result<(), SealError> {
    // 1. 重算载荷哈希（字段级防篡改）
    let expect = compute_anchor_hash(prev_anchor_hash, seal);
    if expect != seal.anchor_hash {
        return Err(SealError::PayloadHashMismatch { seq: seal.seq });
    }
    // 2. 验签名（真实性/防抵赖，v2 载荷串接字节）
    let vk_bytes = hex_decode(verifying_key_hex).ok_or_else(|| SealError::HexInvalid {
        seq: seal.seq,
        reason: String::from("公钥 hex 非法"),
    })?;
    let vk_len = vk_bytes.len();
    let vk_arr: [u8; 32] = vk_bytes.try_into().map_err(|_| SealError::HexInvalid {
        seq: seal.seq,
        reason: format!("公钥长度 != 32: {vk_len}"),
    })?;
    let sig_bytes = hex_decode(&seal.signature).ok_or_else(|| SealError::HexInvalid {
        seq: seal.seq,
        reason: String::from("签名 hex 非法"),
    })?;
    let sig_len = sig_bytes.len();
    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| SealError::HexInvalid {
        seq: seal.seq,
        reason: format!("签名长度 != 64: {sig_len}"),
    })?;
    let vk =
        ed25519_dalek::VerifyingKey::from_bytes(&vk_arr).map_err(|e| SealError::HexInvalid {
            seq: seal.seq,
            reason: format!("公钥非法: {e}"),
        })?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(signature_payload_v2(seal).as_bytes(), &sig)
        .map_err(|_| SealError::SignatureInvalid { seq: seal.seq })
}

/// 锚点链验证（四规则：seq 连续 / 区间无缝 / 哈希链 / 验签）
///
/// `keys`：key_id → 公钥 hex（32 字节 ed25519 公钥的 hex）。
pub fn verify_seal_chain(
    seals: &[AnchorSeal],
    keys: &BTreeMap<String, String>,
) -> Result<(), SealError> {
    let mut prev_hash = String::from(ANCHOR_GENESIS);
    let mut expected_lo: Option<u64> = Some(0);
    for (expected_seq, seal) in (0u64..).zip(seals.iter()) {
        if seal.seq != expected_seq {
            return Err(SealError::ChainBroken {
                seq: seal.seq,
                reason: format!("seq 应为 {expected_seq}（0 起严格 +1）"),
            });
        }
        if let Some(lo) = expected_lo {
            if seal.fact_range.lo != lo {
                return Err(SealError::ChainBroken {
                    seq: seal.seq,
                    reason: format!("区间无缝断裂: lo={} 期望 {lo}", seal.fact_range.lo),
                });
            }
        }
        if seal.fact_range.lo > seal.fact_range.hi {
            return Err(SealError::ChainBroken {
                seq: seal.seq,
                reason: String::from("区间倒挂（lo > hi）"),
            });
        }
        let key_hex = keys
            .get(&seal.key_id)
            .ok_or_else(|| SealError::KeyMissing {
                key_id: seal.key_id.clone(),
            })?;
        verify_seal(seal, &prev_hash, key_hex)?;
        prev_hash = seal.anchor_hash.clone();
        expected_lo = Some(seal.fact_range.hi);
    }
    Ok(())
}
