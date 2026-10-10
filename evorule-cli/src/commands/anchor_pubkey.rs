// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule anchor-pubkey` —— 从私钥种子派生验证公钥（部署配套操作）
//!
//! 部署面（evorule-server `EVORULE_ANCHOR_SEED`）只持有种子；验证方
//! 只需公钥。本命令从种子确定性派生公钥并打印，供安全分发：
//!
//! ```text
//! evorule anchor-keygen --output seed.txt   # 一次性生成
//! evorule anchor-pubkey --seed <64-hex>     # 部署/轮换时导出公钥
//! ```
//!
//! 种子也可从 stdin 读入（`--seed -`，适合 `cat seed.txt |` 形态，
//! 避免进 shell 历史）。

use crate::error::CliError;
use crate::signing::AuditSigner;

/// 执行 anchor-pubkey 子命令
///
/// # 参数
/// - `seed_arg`：64-hex 私钥种子，或 `-` 表示从 stdin 读一行
///
/// # 退出码
/// - 0：成功打印公钥
/// - 1：种子格式非法
pub fn run(seed_arg: &str) -> Result<(), CliError> {
    let seed_hex = if seed_arg == "-" {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
            .map_err(|e| CliError::other(format!("Read stdin failed: {e}")))?;
        line.trim().to_string()
    } else {
        seed_arg.to_string()
    };
    let signer = AuditSigner::from_hex(&seed_hex).map_err(|e| CliError::other(e.to_string()))?;
    println!("{}", crate::signing::hex_encode(&signer.verifying_bytes()));
    Ok(())
}
