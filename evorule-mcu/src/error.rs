// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! MCU kernel error types — no_std, no error ever panics

use alloc::string::String;

/// MCU kernel errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McuError {
    /// Rule pack load/parse failure (postcard decoding or the root shape is
    /// not a rules array)
    Pack(String),
    /// JSON serialization failure (WAL line construction)
    Serde(String),
    /// Hash computation failure (passes through the SSOT `HashError` message)
    Hash(String),
    /// WAL record index out of range
    RecordIndex {
        /// The requested index
        index: usize,
        /// The actual record count
        len: usize,
    },
    /// A rule triggered `io_request`; the IoRequest fact is already on the
    /// chain and run pauses with this error; the board performs the I/O via
    /// [`crate::io::IoBridge`] and calls `resume_io` to continue
    IoPaused {
        /// The on-chain IoRequest fact id
        request_id: u64,
        /// The requested io_type
        io_type: String,
    },
    /// State version counter overflow (u64)
    VersionOverflow,
    /// FactId counter overflow (u64)
    FactIdOverflow,
    /// Flash WAL ring full (fail-visible: the audit chain never silently
    /// drops records; wait for external export/re-erase)
    WalFull,
    /// Flash program violation (a target byte contains a 0→1 bit —
    /// programming only allows 1→0)
    FlashProgramViolated {
        /// The medium offset of the violated byte
        offset: usize,
    },
    /// Flash read/write offset out of range
    FlashRange {
        /// The out-of-range offset
        offset: usize,
    },
    /// WAL medium content corrupted (recovery scan fails visibly: an illegal
    /// shape was found in the chain region)
    WalCorrupted(String),
    /// A single WAL line exceeds the page capacity (lines never span pages;
    /// the page is the write atomicity boundary)
    WalRecordTooLarge {
        /// Line size in bytes (including \n)
        line_len: usize,
        /// Page capacity
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
            McuError::IoPaused {
                request_id,
                io_type,
            } => {
                write!(
                    f,
                    "io paused: IoRequest {request_id} (io_type={io_type}) is on the audit chain; \
                     perform the I/O (IoBridge) and call resume_io to continue"
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
