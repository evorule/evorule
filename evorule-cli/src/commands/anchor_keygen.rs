// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule anchor-keygen` —— 生成 G-A1 审计锚点签名密钥对（一次性运维操作）
//!
//! 产出：
//! - 私钥种子（32 字节，64 位 hex）—— **必须私密保存**，用于配置审计器签名锚点
//! - 公钥（32 字节，64 位 hex）—— 可公开分发，供第三方用 `verify-anchors` 离线验证审计链真实性
//!
//! 密钥不落盘于审计资产/规则库，由使用方注入；本命令仅打印到 stdout（可重定向到安全位置）。

use std::path::Path;

use crate::error::CliError;
use crate::signing::AuditSigner;

/// 执行 anchor-keygen 子命令
///
/// # 退出码
/// - 0：成功生成并打印密钥对
/// - 1：OS 熵源不可用等错误
pub fn run(output: Option<&Path>) -> Result<(), CliError> {
    let (sk_seed_hex, pk_hex) =
        AuditSigner::generate_keys().map_err(|e| CliError::other(e.to_string()))?;

    if let Some(path) = output {
        // 仅写入私钥种子文件（配合 --pubkey 单独分发公钥，避免私钥散落）
        std::fs::write(path, format!("{}\n", sk_seed_hex))
            .map_err(|e| CliError::other(format!("写入私钥文件失败: {e}")))?;
        println!(
            "[OK] 私钥种子已写入: {} （公钥: {}）",
            path.display(),
            pk_hex
        );
        println!("[WARN] 本文件包含私钥种子，请以安全方式保管（建议 chmod 600 或同理权限）");
    } else {
        println!("=== G-A1 审计锚点签名密钥对 ===");
        println!("[SECRET] 私钥种子 (sk_seed_hex): {}", sk_seed_hex);
        println!("[PUBLIC] 公钥 (pk_hex): {}", pk_hex);
        println!("[WARN] 私钥种子请绝对不要泄露/提交到版本库；公钥可分发给验证方");
    }
    Ok(())
}