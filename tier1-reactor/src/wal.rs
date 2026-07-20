// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Write-Ahead Log (WAL) —— 事实审计链的磁盘持久化
//!
//! # 设计依据
//! P0-1（参见 `文档/07_生产级差距.md`）：FactsLog 原本仅内存存储，进程崩溃即丢失。
//! WAL 在每次 `append` 之前先将 Fact 序列化为 JSONL 写入磁盘并 flush，
//! 保证进程崩溃/重启/OOM 后可通过 `recover` 重放事实恢复状态。
//!
//! # WAL 格式（JSONL，每行一条记录）
//! ```json
//! {"version_before": 0, "fact": {"type": "Command", "id": 1, "instruction": {...}}}
//! ```
//!
//! # 持久化级别
//! - `flush`：保证进程崩溃/重启/OOM 不丢数据（P0 风险场景）
//! - `fsync`：保证断电不丢数据（P1+ 优化项，P0 暂不实现，权衡 fsync 的性能损耗）
//!
//! # 序列化策略
//! 不使用 `#[derive(Serialize/Deserialize)]`，原因有二：
//! 1. tier0-tcb 是 `no_std` + 零依赖 crate，不能引入 serde 派生宏
//! 2. 孤儿规则禁止在 tier1 为 tier0 类型实现外部 trait
//!
//! 改用 free function 桥接：`tcb_to_serde`/`serde_to_tcb` 转换
//! `JsonValue ↔ serde_json::Value`，再由 `fact_to_json`/`fact_from_json`
//! 手动处理 7 种 Fact 变体的序列化。
//!
//! # 恢复流程
//! 1. `read_wal(path)` 读取所有 (version_before, Fact) 记录
//! 2. `FactsLog::recover(path)` 重放事实到内存状态（WAL 未挂载，不重复写）
//! 3. 重放完成后以 `WalWriter::append` 模式挂载 WAL，继续追加新事实

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use tier0_tcb::JsonValue;

use crate::fact::{Fact, FactId, IoType};

/// WAL 错误类型
#[derive(Debug)]
pub enum WalError {
    /// 底层 I/O 错误（文件读写失败）
    Io(std::io::Error),
    /// JSON 序列化/反序列化错误
    Json(serde_json::Error),
    /// Fact 反序列化失败（未知类型名/缺字段/字段类型不匹配）
    InvalidFact(String),
}

impl core::fmt::Display for WalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WalError::Io(e) => write!(f, "WAL I/O error: {e}"),
            WalError::Json(e) => write!(f, "WAL JSON error: {e}"),
            WalError::InvalidFact(msg) => write!(f, "WAL invalid fact: {msg}"),
        }
    }
}

impl std::error::Error for WalError {}

impl From<std::io::Error> for WalError {
    fn from(e: std::io::Error) -> Self {
        WalError::Io(e)
    }
}

impl From<serde_json::Error> for WalError {
    fn from(e: serde_json::Error) -> Self {
        WalError::Json(e)
    }
}

/// tier0 `JsonValue` → `serde_json::Value`
///
/// 直接映射，因 tier0 无 Float 类型，故整数/字符串/布尔/null/数组/对象一一对应。
pub fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        JsonValue::String(s) => serde_json::Value::String(s.clone()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), tcb_to_serde(v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

/// `serde_json::Value` → tier0 `JsonValue`
///
/// # Float 处理
/// tier0 无 Float 类型（形式化验证约束）。遇到 serde 的浮点数时，
/// 转为字符串保留精度（`n.to_string()`），避免静默截断。
/// u64 超 i64 范围时同样转字符串。
pub fn serde_to_tcb(v: &serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                // u64 超 i64 范围则转字符串保留值
                match i64::try_from(u) {
                    Ok(i) => JsonValue::Integer(i),
                    Err(_) => JsonValue::String(n.to_string()),
                }
            } else {
                // 浮点数：tier0 无 Float，转字符串保留精度
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s.clone()),
        serde_json::Value::Array(arr) => JsonValue::Array(arr.iter().map(serde_to_tcb).collect()),
        serde_json::Value::Object(map) => {
            let mut obj = BTreeMap::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), serde_to_tcb(v));
            }
            JsonValue::Object(obj)
        }
    }
}

/// `Fact` → `serde_json::Value`
///
/// 每个变体序列化为带 `type` 鉴别字段的 JSON 对象，例如：
/// ```json
/// {"type": "Command", "id": 1, "instruction": {...}}
/// ```
pub fn fact_to_json(fact: &Fact) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    match fact {
        Fact::Command { id, instruction } => {
            obj.insert("type".into(), serde_json::Value::String("Command".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
        Fact::PayloadUpdate { id, path, value } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("PayloadUpdate".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("path".into(), serde_json::Value::String(path.clone()));
            obj.insert("value".into(), tcb_to_serde(value));
        }
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("StateTransition".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert("new_payload".into(), tcb_to_serde(new_payload));
            obj.insert(
                "new_queue".into(),
                serde_json::Value::Array(new_queue.iter().map(tcb_to_serde).collect()),
            );
        }
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => {
            obj.insert("type".into(), serde_json::Value::String("IoRequest".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "io_type".into(),
                serde_json::Value::String(io_type.as_str().into()),
            );
            obj.insert("params".into(), tcb_to_serde(params));
        }
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("IoResponse".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "request_id".into(),
                serde_json::Value::Number(request_id.0.into()),
            );
            obj.insert("result".into(), tcb_to_serde(result));
            obj.insert(
                "error".into(),
                match error {
                    Some(msg) => serde_json::Value::String(msg.clone()),
                    None => serde_json::Value::Null,
                },
            );
        }
        Fact::Stable { id, final_snapshot } => {
            obj.insert("type".into(), serde_json::Value::String("Stable".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("final_snapshot".into(), tcb_to_serde(final_snapshot));
        }
        Fact::Error { id, message } => {
            obj.insert("type".into(), serde_json::Value::String("Error".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("message".into(), serde_json::Value::String(message.clone()));
        }
    }
    serde_json::Value::Object(obj)
}

/// `serde_json::Value` → `Fact`
///
/// 反序列化带 `type` 鉴别字段的 JSON 对象。任何字段缺失或类型不匹配
/// 均返回 `WalError::InvalidFact`，调用方可决定跳过该行或中止恢复。
pub fn fact_from_json(v: &serde_json::Value) -> Result<Fact, WalError> {
    let obj = v
        .as_object()
        .ok_or_else(|| WalError::InvalidFact("fact is not an object".into()))?;
    let type_str = obj
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| WalError::InvalidFact("missing 'type' field".into()))?;
    let id_raw = obj
        .get("id")
        .and_then(|i| i.as_i64())
        .ok_or_else(|| WalError::InvalidFact("missing/invalid 'id' field".into()))?;
    let id = FactId(id_raw as u64);

    match type_str {
        "Command" => {
            let instruction = obj
                .get("instruction")
                .ok_or_else(|| WalError::InvalidFact("Command missing 'instruction'".into()))?;
            Ok(Fact::Command {
                id,
                instruction: serde_to_tcb(instruction),
            })
        }
        "PayloadUpdate" => {
            let path = obj
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| WalError::InvalidFact("PayloadUpdate missing 'path'".into()))?;
            let value = obj
                .get("value")
                .ok_or_else(|| WalError::InvalidFact("PayloadUpdate missing 'value'".into()))?;
            Ok(Fact::PayloadUpdate {
                id,
                path: path.into(),
                value: serde_to_tcb(value),
            })
        }
        "StateTransition" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_i64())
                .ok_or_else(|| WalError::InvalidFact("StateTransition missing 'cause'".into()))?;
            let new_payload = obj.get("new_payload").ok_or_else(|| {
                WalError::InvalidFact("StateTransition missing 'new_payload'".into())
            })?;
            let new_queue_arr =
                obj.get("new_queue")
                    .and_then(|q| q.as_array())
                    .ok_or_else(|| {
                        WalError::InvalidFact("StateTransition missing 'new_queue'".into())
                    })?;
            let new_queue: Vec<JsonValue> = new_queue_arr.iter().map(serde_to_tcb).collect();
            Ok(Fact::StateTransition {
                id,
                cause: FactId(cause_raw as u64),
                new_payload: serde_to_tcb(new_payload),
                new_queue,
            })
        }
        "IoRequest" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_i64())
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'cause'".into()))?;
            let io_type_str = obj
                .get("io_type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'io_type'".into()))?;
            let io_type = IoType::parse(io_type_str)
                .ok_or_else(|| WalError::InvalidFact(format!("unknown io_type: {io_type_str}")))?;
            let params = obj
                .get("params")
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'params'".into()))?;
            Ok(Fact::IoRequest {
                id,
                cause: FactId(cause_raw as u64),
                io_type,
                params: serde_to_tcb(params),
            })
        }
        "IoResponse" => {
            let request_id_raw = obj
                .get("request_id")
                .and_then(|r| r.as_i64())
                .ok_or_else(|| WalError::InvalidFact("IoResponse missing 'request_id'".into()))?;
            let result = obj
                .get("result")
                .ok_or_else(|| WalError::InvalidFact("IoResponse missing 'result'".into()))?;
            let error = match obj.get("error") {
                Some(serde_json::Value::Null) | None => None,
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(_) => {
                    return Err(WalError::InvalidFact(
                        "IoResponse 'error' must be string or null".into(),
                    ))
                }
            };
            Ok(Fact::IoResponse {
                id,
                request_id: FactId(request_id_raw as u64),
                result: serde_to_tcb(result),
                error,
            })
        }
        "Stable" => {
            let final_snapshot = obj
                .get("final_snapshot")
                .ok_or_else(|| WalError::InvalidFact("Stable missing 'final_snapshot'".into()))?;
            Ok(Fact::Stable {
                id,
                final_snapshot: serde_to_tcb(final_snapshot),
            })
        }
        "Error" => {
            let message = obj
                .get("message")
                .and_then(|m| m.as_str())
                .ok_or_else(|| WalError::InvalidFact("Error missing 'message'".into()))?;
            Ok(Fact::Error {
                id,
                message: message.into(),
            })
        }
        other => Err(WalError::InvalidFact(format!("unknown fact type: {other}"))),
    }
}

/// WAL 文件轮换策略默认值（P03）
pub const DEFAULT_MAX_WAL_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// WAL 写入器
///
/// 持有 `BufWriter<File>`，每次 `append` 后立即 `flush`，
/// 保证 write-ahead 语义（进程崩溃时磁盘已落盘）。
///
/// # fsync 支持（P02）
/// 可选的 fsync 支持，启用后在每次 flush 后执行 `sync_all()`，
/// 确保断电时数据不丢失（性能开销较大，默认禁用）。
///
/// # 文件轮换（P03）
/// 支持按大小自动轮换，单个文件超过 `max_size_bytes` 时自动创建新文件，
/// 文件名格式为 `session_N.wal`（主文件）和 `session_N.wal.1`、`session_N.wal.2` 等（轮换文件）。
pub struct WalWriter {
    writer: BufWriter<File>,
    path: PathBuf,
    max_size_bytes: u64,
    current_size_bytes: u64,
    fsync_on_flush: bool,
    file_sequence: u64,
}

impl WalWriter {
    fn build_rotated_path(path: &Path, sequence: u64) -> PathBuf {
        if sequence == 0 {
            path.to_path_buf()
        } else {
            let mut p = path.to_path_buf();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if ext.is_empty() {
                p.set_file_name(format!("{}.{}", stem, sequence));
            } else {
                p.set_file_name(format!("{}.{}.{}", stem, sequence, ext));
            }
            p
        }
    }

    fn open_file(path: &Path, sequence: u64, truncate: bool) -> Result<(File, PathBuf), WalError> {
        let rotated_path = Self::build_rotated_path(path, sequence);
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if truncate {
            options.truncate(true);
        } else {
            options.append(true);
        }
        let file = options.open(&rotated_path)?;
        Ok((file, rotated_path))
    }

    /// 创建新 WAL 文件（truncate 已有文件）
    ///
    /// 用于 `FactsLog::with_wal` 全新启动场景。
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, WalError> {
        Self::create_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, false)
    }

    /// 创建新 WAL 文件并指定 fsync 选项（P02）
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    pub fn create_with_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, WalError> {
        Self::create_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// 创建新 WAL 文件并指定轮换和 fsync 选项（P03）
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `max_size_bytes`: 单个文件最大大小，达到后自动轮换（0 表示不轮换）
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    pub fn create_with_options<P: AsRef<Path>>(
        path: P,
        max_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, WalError> {
        let path_buf = path.as_ref().to_path_buf();
        let (file, _) = Self::open_file(&path_buf, 0, true)?;
        Ok(Self {
            writer: BufWriter::new(file),
            path: path_buf,
            max_size_bytes,
            current_size_bytes: 0,
            fsync_on_flush: fsync,
            file_sequence: 0,
        })
    }

    /// 以追加模式打开 WAL 文件
    ///
    /// 用于 `FactsLog::recover` 后继续写入：文件已存在则追加，
    /// 不存在则创建（recover 空文件场景）。
    pub fn append<P: AsRef<Path>>(path: P) -> Result<Self, WalError> {
        Self::append_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, false)
    }

    /// 以追加模式打开 WAL 文件并指定 fsync 选项（P02）
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    pub fn append_with_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, WalError> {
        Self::append_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// 以追加模式打开 WAL 文件并指定轮换和 fsync 选项（P03）
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `max_size_bytes`: 单个文件最大大小，达到后自动轮换（0 表示不轮换）
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    pub fn append_with_options<P: AsRef<Path>>(
        path: P,
        max_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, WalError> {
        let path_buf = path.as_ref().to_path_buf();

        let mut sequence = 0;
        loop {
            let rotated_path = Self::build_rotated_path(&path_buf, sequence);
            if !rotated_path.exists() {
                if sequence > 0 {
                    sequence -= 1;
                }
                break;
            }
            sequence += 1;
        }

        let (file, _) = Self::open_file(&path_buf, sequence, false)?;

        let current_size = if sequence == 0 && path_buf.exists() {
            std::fs::metadata(&path_buf).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        Ok(Self {
            writer: BufWriter::new(file),
            path: path_buf,
            max_size_bytes,
            current_size_bytes: current_size,
            fsync_on_flush: fsync,
            file_sequence: sequence,
        })
    }

    fn rotate(&mut self) -> Result<(), WalError> {
        self.file_sequence += 1;
        let (file, _) = Self::open_file(&self.path, self.file_sequence, true)?;
        self.writer = BufWriter::new(file);
        self.current_size_bytes = 0;
        Ok(())
    }

    /// 追加一条记录：`{"version_before": N, "fact": {...}}`
    ///
    /// 调用后立即 `flush`，若启用 fsync 则额外执行 `sync_all()`，
    /// 保证进程崩溃和断电时数据不丢失。
    ///
    /// 若启用文件轮换（`max_size_bytes > 0`），当当前文件大小超过阈值时自动创建新文件。
    pub fn append_record(&mut self, version_before: u64, fact: &Fact) -> Result<(), WalError> {
        let mut record = serde_json::Map::new();
        record.insert(
            "version_before".into(),
            serde_json::Value::Number(version_before.into()),
        );
        record.insert("fact".into(), fact_to_json(fact));
        let line = serde_json::to_string(&serde_json::Value::Object(record))?;

        let line_bytes = line.len() as u64 + 1;

        if self.max_size_bytes > 0
            && self.current_size_bytes > 0
            && self.current_size_bytes + line_bytes > self.max_size_bytes
        {
            self.rotate()?;
        }

        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        if self.fsync_on_flush {
            self.writer.get_mut().sync_all()?;
        }

        self.current_size_bytes += line_bytes;

        Ok(())
    }
}

fn read_wal_file<P: AsRef<Path>>(
    path: P,
    base_line_no: usize,
) -> Result<Vec<(u64, Fact)>, WalError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for (line_idx, line) in reader.lines().enumerate() {
        let line_no = base_line_no + line_idx;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|e| WalError::InvalidFact(format!("line {line_no}: JSON parse error: {e}")))?;
        let obj = value
            .as_object()
            .ok_or_else(|| WalError::InvalidFact(format!("line {line_no}: not an object")))?;
        let version_before = obj
            .get("version_before")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                WalError::InvalidFact(format!("line {line_no}: missing version_before"))
            })?;
        let fact_value = obj
            .get("fact")
            .ok_or_else(|| WalError::InvalidFact(format!("line {line_no}: missing fact")))?;
        let fact = fact_from_json(fact_value)?;
        records.push((version_before, fact));
    }
    Ok(records)
}

/// 读取 WAL 文件，返回所有 (version_before, Fact) 记录
///
/// 支持单文件和多文件轮换格式（P03）：
/// - 单文件：`session_N.wal`
/// - 多文件：`session_N.wal`、`session_N.wal.1`、`session_N.wal.2` 等
///
/// 用于 `FactsLog::recover`：读取 → 重放事实（不写 WAL）→ 挂载 WAL 继续追加。
///
/// # 向后兼容性（P03）
/// 完全兼容旧版单文件 WAL：
/// - 自动检测：读取时先检查主文件（无序列号后缀）是否存在
/// - 顺序读取：按序列号从小到大依次读取轮换文件
/// - 无缝升级：启用轮换后，新建的 WAL 会自动使用轮换策略，旧 WAL 文件不受影响
/// - 降级支持：若只存在主文件（旧版格式），行为与旧版完全一致
///
/// # 错误处理
/// - 文件不存在 → `WalError::Io`
/// - 任意行 JSON 解析失败 → `WalError::InvalidFact`（带行号）
/// - 空行自动跳过
pub fn read_wal<P: AsRef<Path>>(path: P) -> Result<Vec<(u64, Fact)>, WalError> {
    let path_buf = path.as_ref().to_path_buf();
    let mut records = Vec::new();
    let mut line_no = 0;
    let mut found_any_file = false;

    if path_buf.exists() {
        records.extend(read_wal_file(&path_buf, line_no)?);
        line_no += std::fs::read_to_string(&path_buf)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        found_any_file = true;
    }

    let mut sequence = 1;
    loop {
        let rotated_path = {
            let mut p = path_buf.clone();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if ext.is_empty() {
                p.set_file_name(format!("{}.{}", stem, sequence));
            } else {
                p.set_file_name(format!("{}.{}.{}", stem, sequence, ext));
            }
            p
        };

        if !rotated_path.exists() {
            break;
        }

        records.extend(read_wal_file(&rotated_path, line_no)?);
        line_no += std::fs::read_to_string(&rotated_path)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        found_any_file = true;
        sequence += 1;
    }

    if !found_any_file {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "WAL file not found").into());
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::{Fact, FactId, IoType};
    use tier0_tcb::JsonValue;

    // === tcb_to_serde / serde_to_tcb 单元测试 ===

    #[test]
    fn test_tcb_serde_null_roundtrip() {
        let v = JsonValue::Null;
        let s = tcb_to_serde(&v);
        assert_eq!(s, serde_json::Value::Null);
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_bool_roundtrip() {
        for b in [true, false] {
            let v = JsonValue::Bool(b);
            let s = tcb_to_serde(&v);
            assert_eq!(s, serde_json::Value::Bool(b));
            assert_eq!(serde_to_tcb(&s), v);
        }
    }

    #[test]
    fn test_tcb_serde_integer_roundtrip() {
        for i in [0i64, 1, -1, i64::MAX, i64::MIN, 42, -99] {
            let v = JsonValue::Integer(i);
            let s = tcb_to_serde(&v);
            assert_eq!(s.as_i64(), Some(i));
            assert_eq!(serde_to_tcb(&s), v);
        }
    }

    #[test]
    fn test_tcb_serde_string_roundtrip() {
        let v = JsonValue::String("hello 世界".into());
        let s = tcb_to_serde(&v);
        assert_eq!(s, serde_json::Value::String("hello 世界".into()));
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_array_roundtrip() {
        let v = JsonValue::Array(vec![
            JsonValue::Null,
            JsonValue::Integer(1),
            JsonValue::String("x".into()),
        ]);
        let s = tcb_to_serde(&v);
        assert!(s.is_array());
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_object_roundtrip() {
        let v = JsonValue::object_from_pairs(&[
            ("a", JsonValue::Integer(1)),
            ("b", JsonValue::String("y".into())),
            ("c", JsonValue::Array(vec![JsonValue::Bool(true)])),
        ]);
        let s = tcb_to_serde(&v);
        assert!(s.is_object());
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_serde_to_tcb_float_becomes_string() {
        // tier0 无 Float，浮点数应转为字符串保留精度
        let s: serde_json::Value = serde_json::from_str("3.14").unwrap();
        let tcb = serde_to_tcb(&s);
        match tcb {
            JsonValue::String(ref _str) => {}
            ref other => panic!("expected String for float, got {other:?}"),
        }
    }

    #[test]
    fn test_serde_to_tcb_u64_overflow_becomes_string() {
        // u64 超 i64 范围时转字符串
        let s: serde_json::Value = serde_json::from_str(&format!("{}", u64::MAX)).unwrap();
        let tcb = serde_to_tcb(&s);
        match tcb {
            JsonValue::String(ref _str) => {}
            ref other => panic!("expected String for u64 overflow, got {other:?}"),
        }
    }

    // === fact_to_json / fact_from_json 7 种变体往返测试 ===

    fn assert_fact_roundtrip(fact: &Fact) {
        let json = fact_to_json(fact);
        let restored = fact_from_json(&json).expect("roundtrip should succeed");
        assert_eq!(restored, *fact);
    }

    #[test]
    fn test_fact_command_roundtrip() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("increment".into())),
                (
                    "params",
                    JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
                ),
            ]),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_payload_update_roundtrip() {
        let fact = Fact::PayloadUpdate {
            id: FactId(7),
            path: "user.profile.name".into(),
            value: JsonValue::String("alice".into()),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_state_transition_roundtrip() {
        let fact = Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![JsonValue::String("instr1".into()), JsonValue::Integer(99)],
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_io_request_roundtrip() {
        for io_type in [
            IoType::CALL_EXTERNAL,
            IoType::QUERY_DB,
            IoType::HTTP_GET,
            IoType::SAVE_MEMORY,
            IoType::CALL_SERVICE,
        ] {
            let fact = Fact::IoRequest {
                id: FactId(3),
                cause: FactId(2),
                io_type,
                params: JsonValue::object_from_pairs(&[("prompt", JsonValue::String("hi".into()))]),
            };
            assert_fact_roundtrip(&fact);
        }
    }

    #[test]
    fn test_fact_io_response_success_roundtrip() {
        let fact = Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::String("llm reply".into()),
            error: None,
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_io_response_error_roundtrip() {
        let fact = Fact::IoResponse {
            id: FactId(5),
            request_id: FactId(3),
            result: JsonValue::Null,
            error: Some("timeout after 30s".into()),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_stable_roundtrip() {
        let fact = Fact::Stable {
            id: FactId(6),
            final_snapshot: JsonValue::object_from_pairs(&[("done", JsonValue::Bool(true))]),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_error_roundtrip() {
        let fact = Fact::Error {
            id: FactId(8),
            message: "max rounds exceeded".into(),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_from_json_unknown_type() {
        let json = serde_json::json!({"type": "Unknown", "id": 1});
        let result = fact_from_json(&json);
        assert!(matches!(result, Err(WalError::InvalidFact(_))));
    }

    #[test]
    fn test_fact_from_json_missing_type() {
        let json = serde_json::json!({"id": 1});
        let result = fact_from_json(&json);
        assert!(matches!(result, Err(WalError::InvalidFact(_))));
    }

    // === WalWriter + read_wal 集成测试 ===

    fn temp_wal_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "evorule_wal_test_{name}_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn test_wal_create_append_read_roundtrip() {
        let path = temp_wal_path("roundtrip");
        let mut writer = WalWriter::create(&path).unwrap();
        let facts = vec![
            (
                0u64,
                Fact::Command {
                    id: FactId(1),
                    instruction: JsonValue::object_from_pairs(&[(
                        "type",
                        JsonValue::String("increment".into()),
                    )]),
                },
            ),
            (
                0u64,
                Fact::StateTransition {
                    id: FactId(2),
                    cause: FactId(1),
                    new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
                    new_queue: vec![],
                },
            ),
            (
                1u64,
                Fact::Stable {
                    id: FactId(3),
                    final_snapshot: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
                },
            ),
        ];
        for (vb, f) in &facts {
            writer.append_record(*vb, f).unwrap();
        }
        drop(writer);

        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), facts.len());
        for (i, ((vb_expected, f_expected), (vb_actual, f_actual))) in
            facts.iter().zip(records.iter()).enumerate()
        {
            assert_eq!(vb_actual, vb_expected, "version_before mismatch at {i}");
            assert_eq!(f_actual, f_expected, "fact mismatch at {i}");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_append_mode_continues_existing_file() {
        let path = temp_wal_path("append_mode");

        // 第一次：create + 写 2 条
        let mut w1 = WalWriter::create(&path).unwrap();
        w1.append_record(
            0,
            &Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
        )
        .unwrap();
        w1.append_record(
            0,
            &Fact::Stable {
                id: FactId(2),
                final_snapshot: JsonValue::empty_object(),
            },
        )
        .unwrap();
        drop(w1);

        // 第二次：append 模式继续写 1 条
        let mut w2 = WalWriter::append(&path).unwrap();
        w2.append_record(
            0,
            &Fact::Error {
                id: FactId(3),
                message: "test".into(),
            },
        )
        .unwrap();
        drop(w2);

        // 读取应得 3 条
        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), 3);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_wal_skips_blank_lines() {
        let path = temp_wal_path("blank_lines");
        let mut w = WalWriter::create(&path).unwrap();
        w.append_record(
            0,
            &Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
        )
        .unwrap();
        drop(w);

        // 手动追加空行
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f).unwrap();
        writeln!(f, "   ").unwrap();
        drop(f);

        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), 1, "blank lines should be skipped");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_wal_nonexistent_file_returns_io_error() {
        let path = temp_wal_path("nonexistent");
        let result = read_wal(&path);
        assert!(matches!(result, Err(WalError::Io(_))));
    }

    #[test]
    fn test_wal_error_display() {
        let e = WalError::InvalidFact("bad fact".into());
        assert!(format!("{e}").contains("bad fact"));

        let e = WalError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert!(format!("{e}").contains("missing"));
    }
}
