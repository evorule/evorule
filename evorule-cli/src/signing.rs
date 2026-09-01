// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 审计锚点签名工具（G-A1 真实性 / 防抵赖）—— CLI 侧最小自包含实现
//!
//! # 背景
//!
//! evorule-governance 的审计链用 BLAKE3 哈希保证"完整性/被动篡改"，但不提供"真实性/防抵赖"。
//! G-A1 在该链之上引入 **ed25519（RFC 8032）确定性签名锚点**：用私钥对审计链尾 `last_hash`
//! 签名，公钥随导出物分发即可供第三方离线验证"审计链确由私钥持有者生成"。
//!
//! # 本模块定位
//!
//! 遵循 evorule-cli「不引入 evorule-governance」的架构约束（CLI 定位 tier0+tier1、musl 静态），
//! 本模块**复制** governance `signing.rs` 的最小实现（与已复制的 `hash.rs` 同思路），
//! 仅提供 CLI 需要的：密钥生成 + 验签 + hex 编解码。锚点载荷的重算在 `verify-anchors` 命令中。
//!
//! # 密钥纪律
//!
//! 私钥不落盘于审计资产/规则库，由调用方注入 32 字节种子；`generate_keys` 仅供一次性运维使用。

use ed25519_dalek::{Signature, Signer, Verifier};

/// 签名相关错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// 签名/验证输入非法（种子长度、验证密钥字节、签名长度、hex 解析）
    Invalid(String),
    /// 生成随机种子失败（OS 熵源不可用）
    Randomness(&'static str),
}

impl core::fmt::Display for SignError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SignError::Invalid(msg) => write!(f, "审计签名: 非法输入: {msg}"),
            SignError::Randomness(src) => write!(f, "审计签名: 随机种子失败: {src}"),
        }
    }
}

impl std::error::Error for SignError {}

/// ed25519 审计签名器（持有私钥）
///
/// 同一私钥 + 同一载荷 → 同一签名（RFC 8032 确定性 nonce，无 RNG），与 evorule 的确定性执行纪律兼容。
#[derive(Debug, Clone)]
pub struct AuditSigner {
    signing: ed25519_dalek::SigningKey,
    verifying: ed25519_dalek::VerifyingKey,
}

impl AuditSigner {
    /// 从 32 字节私钥种子构造签名器
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// 从 64 位 hex 私钥种子字符串构造签名器
    pub fn from_hex(seed_hex: &str) -> Result<Self, SignError> {
        let bytes = hex_decode(seed_hex)?;
        if bytes.len() != 32 {
            return Err(SignError::Invalid(format!(
                "私钥种子长度 != 32: {}",
                bytes.len()
            )));
        }
        // len 已校验 == 32，无 panic-prone 拷贝
        let mut seed = [0u8; 32];
        for (dst, src) in seed.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
        Ok(Self::from_bytes(seed))
    }

    /// 生成一对新密钥（一次性运维操作）
    ///
    /// 返回 `(sk_seed_hex, pk_hex)`：`sk_seed_hex` 为 32 字节私钥种子（需私密保存），
    /// `pk_hex` 为 32 字节压缩公钥（可公开分发供验证）。
    pub fn generate_keys() -> Result<(String, String), SignError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed)
            .map_err(|_| SignError::Randomness("OS 熵源不可用"))?;
        let signer = Self::from_bytes(seed);
        Ok((hex_encode(&seed), hex_encode(&signer.verifying_bytes())))
    }

    /// 32 字节压缩公钥（供验证方使用）
    pub fn verifying_bytes(&self) -> [u8; 32] {
        self.verifying.to_bytes()
    }

    /// 对载荷做确定性签名，返回 64 字节签名
    pub fn signature_bytes(&self, payload: &[u8]) -> [u8; 64] {
        let sig: Signature = self.signing.sign(payload);
        sig.to_bytes()
    }
}

/// 用公钥验证签名（供导出物离线校验使用）
///
/// `verifying_bytes`：32 字节压缩公钥；`payload`：被签名的载荷；`sig_bytes`：64 字节签名。
/// 返回 `Ok(true)` 表示签名有效且完整；`Ok(false)` 表示签名不匹配；`Err` 表示输入非法。
pub fn verify_signature(
    verifying_bytes: [u8; 32],
    payload: &[u8],
    sig_bytes: &[u8; 64],
) -> Result<bool, SignError> {
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&verifying_bytes)
        .map_err(|e| SignError::Invalid(format!("公钥非法: {e}")))?;
    let sig = Signature::from_bytes(sig_bytes);
    Ok(verifying.verify(payload, &sig).is_ok())
}

/// 十六进制编码
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 十六进制解码
pub(crate) fn hex_decode(hex: &str) -> Result<Vec<u8>, SignError> {
    if hex.len() % 2 != 0 {
        return Err(SignError::Invalid(format!("hex 长度为奇数: {}", hex.len())));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push(((h << 4) | l) as u8),
            _ => return Err(SignError::Invalid("hex 含非十六进制字符".into())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_signature_is_deterministic() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let payload = b"audit-anchor-payload";
        assert_eq!(signer.signature_bytes(payload), signer.signature_bytes(payload));
    }

    #[test]
    fn test_sign_and_verify_ok() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let pk = signer.verifying_bytes();
        let payload = b"hello-audit";
        let sig = signer.signature_bytes(payload);
        assert!(verify_signature(pk, payload, &sig).unwrap());
    }

    #[test]
    fn test_verify_rejects_tampered_payload() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let pk = signer.verifying_bytes();
        let sig = signer.signature_bytes(b"hello");
        assert!(!verify_signature(pk, b"hellO", &sig).unwrap());
    }

    #[test]
    fn test_hex_roundtrip() {
        let bytes = [0u8, 1, 0xff, 0x10, 0xab, 0xcd];
        let hex = hex_encode(&bytes);
        assert_eq!(hex_decode(&hex).unwrap(), bytes);
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn test_generate_keys_produces_hex() {
        let (sk, pk) = AuditSigner::generate_keys().unwrap();
        assert_eq!(sk.len(), 64);
        assert_eq!(pk.len(), 64);
        // 公钥由私钥正确派生
        let signer = AuditSigner::from_hex(&sk).unwrap();
        assert_eq!(signer.verifying_bytes(), hex_decode(&pk).unwrap().as_slice());
    }
}