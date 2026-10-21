// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 受控恢复：将 WAL 文件截断至最后一条完整行（半行拒收场景的明确出路）
//!
//! 流程（默认干跑，`--confirm` 才落盘）：
//! 1. 读文件字节，定位最后一个换行符——其后若还有非空字节即为半行；
//! 2. 对截断后前缀跑全链三查，输出前缀链验证报告（链尾/记录数）；
//! 3. `--confirm`：先写 `<file>.partial.bak` 备份（含被截断的半行），再以截断
//!    前缀覆写原文件；干跑模式仅打印将执行的动作。
//!
//! 定位：审计链哨兵退出码 5 的处置指引。半行即崩溃窗口内的不完整写入，
//! 截断不损失任何已确认事实（write-ahead 语义：完整行=已确认）。

use std::path::{Path, PathBuf};

use clap::Args;
use evorule_reactor::{chain_step, fact_hash, read_wal_with_hash};

use crate::error::CliError;

#[derive(Debug, Args)]
pub struct TruncatePartialLine {
    /// WAL 文件（含半行）
    pub wal_file: PathBuf,
    /// 确认落盘（默认干跑：仅验证与打印）
    #[arg(long)]
    pub confirm: bool,
}

pub fn run(cmd: &TruncatePartialLine) -> Result<(), CliError> {
    let path = &cmd.wal_file;
    let data = std::fs::read(path).map_err(|e| CliError::other(format!("read: {e}")))?;

    let last_nl = data
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or_else(|| CliError::other("文件无完整行（无换行符），拒绝处理"))?;
    let trailing = &data[last_nl + 1..];
    if trailing.iter().all(|&b| b.is_ascii_whitespace()) || trailing.is_empty() {
        println!(
            "文件已以完整行结尾，无需截断（尾随空白 {} 字节属正常）",
            trailing.len()
        );
        return Ok(());
    }
    let prefix = &data[..=last_nl];
    println!(
        "检测到半行：{} 字节不完整尾段将被截断（前缀保留 {} 字节 / {} 行）",
        trailing.len(),
        prefix.len(),
        prefix.iter().filter(|&&b| b == b'\n').count()
    );

    // 前缀全链验证（复用哨兵同口径三查）
    let tmp = std::env::temp_dir().join(format!(
        "evorule_truncate_prefix_{}_{}",
        std::process::id(),
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, prefix).map_err(|e| CliError::other(format!("temp write: {e}")))?;
    let verify = (|| -> Result<(usize, String), String> {
        let records = read_wal_with_hash(&tmp).map_err(|e| e.to_string())?;
        let mut prev = "genesis".to_string();
        for rec in &records {
            let content = fact_hash(&rec.fact).map_err(|e| e.to_string())?;
            if let Some(stored) = &rec.content_hash {
                if stored != &content {
                    return Err(format!("content_hash mismatch at fact {}", rec.fact.id().0));
                }
            }
            if let Some(stored_prev) = &rec.prev_hash {
                if stored_prev != &prev {
                    return Err(format!("chain broken at fact {}", rec.fact.id().0));
                }
            }
            let chain = chain_step(&prev, &content);
            if let Some(stored_chain) = &rec.chain_hash {
                if stored_chain != &chain {
                    return Err(format!("chain_hash mismatch at fact {}", rec.fact.id().0));
                }
            }
            prev = chain;
        }
        Ok((records.len(), prev))
    })();
    let _ = std::fs::remove_file(&tmp);
    match verify {
        Ok((n, tail)) => println!(
            "前缀链验证 PASS：{n} 条记录，链尾 {}…",
            &tail[..16.min(tail.len())]
        ),
        Err(e) => {
            return Err(CliError::other(format!(
                "前缀链验证 FAIL（截断无济于事，文件存在更早破坏）: {e}"
            )))
        }
    }

    if !cmd.confirm {
        println!(
            "干跑模式（未落盘）。确认后执行：evorule truncate-partial-line {} --confirm",
            path.display()
        );
        return Ok(());
    }

    // 备份（含半行的原始文件）→ 覆写截断前缀
    let bak = backup_path(path);
    std::fs::write(&bak, &data).map_err(|e| CliError::other(format!("backup write: {e}")))?;
    std::fs::write(path, prefix).map_err(|e| CliError::other(format!("truncate write: {e}")))?;
    println!(
        "已截断：{}（备份：{}，含被截断半行，供取证）",
        path.display(),
        bak.display()
    );
    Ok(())
}

fn backup_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".partial.bak");
    PathBuf::from(s)
}
