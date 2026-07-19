// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
#![forbid(unsafe_code)]
//! Memory I/O Handler —— 基于 `tokio::fs` 实现持久化键值存储。
//!
//! - 写模式：参数包含 `value` 字段时，将内容写入 `base_dir/<key>` 文件，返回 `JsonValue::Bool(true)`。
//! - 读模式：参数不包含 `value` 字段时，读取 `base_dir/<key>` 文件内容，返回 `JsonValue::String(content)`。
//!
//! 通过文件系统实现简单的持久化记忆，适用于规则上下文、缓存等场景。

use std::path::PathBuf;
use std::time::Duration;

use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 单次文件 I/O 超时（P0-2：Memory 5s，防止 NFS/网络文件系统卡住）
const MEMORY_TIMEOUT: Duration = Duration::from_secs(5);

/// Memory 处理器
///
/// 以文件系统为后端的键值存储。所有键被映射为 `base_dir` 下的文件路径。
pub struct MemoryHandler {
    /// 存储根目录
    base_dir: PathBuf,
}

impl MemoryHandler {
    /// 创建新的 Memory 处理器。
    ///
    /// # 参数
    /// - `base_dir`: 存储根目录，所有键将作为该目录下的文件。
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// 解析键对应的文件路径。
    ///
    /// 为防止路径遍历攻击，对键中的路径分隔符进行替换，
    /// 确保最终路径始终位于 `base_dir` 之内。
    fn resolve_path(&self, key: &str) -> PathBuf {
        // 将可能用于路径穿越的分隔符替换为下划线
        let safe_key = key.replace(['/', '\\'], "_").replace("..", "_");
        self.base_dir.join(safe_key)
    }
}

impl IoHandler for MemoryHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 key（必需）
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: key".to_string())?;

        let path = self.resolve_path(key);

        // 根据 value 是否存在区分写/读模式
        if let Some(value) = params.get("value") {
            // 写模式：value 必须为字符串
            let content = value
                .as_str()
                .ok_or_else(|| "param 'value' must be a string".to_string())?;

            // 确保父目录存在（P0-2：5s 超时）
            if let Some(parent) = path.parent() {
                tokio::time::timeout(MEMORY_TIMEOUT, tokio::fs::create_dir_all(parent))
                    .await
                    .map_err(|_| {
                        format!("create dir timed out after {}s", MEMORY_TIMEOUT.as_secs())
                    })?
                    .map_err(|e| format!("create dir failed: {e}"))?;
            }

            // 写入文件（P0-2：5s 超时）
            tokio::time::timeout(MEMORY_TIMEOUT, tokio::fs::write(&path, content))
                .await
                .map_err(|_| format!("write file timed out after {}s", MEMORY_TIMEOUT.as_secs()))?
                .map_err(|e| format!("write file failed: {e}"))?;

            Ok(JsonValue::Bool(true))
        } else {
            // 读模式（P0-2：5s 超时）
            let content = tokio::time::timeout(MEMORY_TIMEOUT, tokio::fs::read_to_string(&path))
                .await
                .map_err(|_| format!("read file timed out after {}s", MEMORY_TIMEOUT.as_secs()))?
                .map_err(|e| format!("read file failed: {e}"))?;
            Ok(JsonValue::String(content))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path_sanitizes_traversal() {
        let handler = MemoryHandler::new(PathBuf::from("/tmp/mem"));
        let p = handler.resolve_path("../etc/passwd");
        // 路径穿越字符应被替换
        let s = p.to_string_lossy();
        assert!(!s.contains(".."));
    }

    #[test]
    fn test_resolve_path_replaces_slashes() {
        let handler = MemoryHandler::new(PathBuf::from("/tmp/mem"));
        let p = handler.resolve_path("a/b/c");
        let s = p.to_string_lossy();
        assert!(!s.contains("a/b/c"));
    }
}
