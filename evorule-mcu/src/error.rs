// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! MCU 内核错误类型——no_std，所有错误不 panic

use alloc::string::String;

/// MCU 内核错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McuError {
    /// 规则包加载/解析失败（postcard 解码或根形状非规则数组）
    Pack(String),
    /// JSON 序列化失败（WAL 行构建）
    Serde(String),
    /// 哈希计算失败（透传 SSOT `HashError` 消息）
    Hash(String),
    /// WAL 记录下标越界
    RecordIndex {
        /// 请求的下标
        index: usize,
        /// 实际记录数
        len: usize,
    },
    /// `io_request` 元指令在本批不受支持（fail-visible；IoRequest 事实已入链）
    IoUnsupported {
        /// 已入链的 IoRequest 事实 id
        request_id: u64,
        /// 请求的 io_type
        io_type: String,
    },
    /// 状态版本计数溢出（u64）
    VersionOverflow,
    /// FactId 计数溢出（u64）
    FactIdOverflow,
    /// Flash WAL 环满（fail-visible：审计链不静默丢，等待外部导出/重擦）
    WalFull,
    /// Flash 编程违例（目标字节含 0→1 位——编程只允许 1→0）
    FlashProgramViolated {
        /// 违例的介质偏移
        offset: usize,
    },
    /// Flash 读写偏移越界
    FlashRange {
        /// 越界的偏移
        offset: usize,
    },
    /// WAL 介质内容损坏（恢复扫描 fail-visible：链区出现非法形态）
    WalCorrupted(String),
    /// 单条 WAL 行超出页容量（行不跨页，页是写入原子边界）
    WalRecordTooLarge {
        /// 行字节数（含 \n）
        line_len: usize,
        /// 页容量
        page_size: usize,
    },
}

impl core::fmt::Display for McuError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            McuError::Pack(msg) => write!(f, "rule pack error: {msg}"),
            McuError::Serde(msg) => write!(f, "serialize error: {msg}"),
            McuError::Hash(msg) => write!(f, "hash error: {msg}"),
            McuError::RecordIndex { index, len } => {
                write!(f, "WAL record index {index} out of range (len {len})")
            }
            McuError::IoUnsupported {
                request_id,
                io_type,
            } => {
                write!(
                    f,
                    "io_request unsupported in MCU kernel (request_id={request_id}, io_type={io_type}); \
                     IoRequest fact is on the chain, I/O resume lands with the hardware batch"
                )
            }
            McuError::VersionOverflow => write!(f, "state version overflow"),
            McuError::FactIdOverflow => write!(f, "fact id overflow"),
            McuError::WalFull => write!(
                f,
                "flash WAL ring full; audit chain never silently drops records, \
                 export or erase via the operations path first"
            ),
            McuError::FlashProgramViolated { offset } => {
                write!(f, "flash program violated 1->0 rule at offset {offset}")
            }
            McuError::FlashRange { offset } => {
                write!(f, "flash offset {offset} out of range")
            }
            McuError::WalCorrupted(msg) => write!(f, "wal medium corrupted: {msg}"),
            McuError::WalRecordTooLarge {
                line_len,
                page_size,
            } => {
                write!(f, "wal line {line_len} bytes exceeds page size {page_size}")
            }
        }
    }
}

impl core::error::Error for McuError {}
