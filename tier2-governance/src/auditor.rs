//! 审计器 - 基于 FactsLog 构建审计链
//!
//! # 功能
//! - 定期从 FactsLog 读取新事实
//! - 计算每个 Fact 的哈希，维护哈希链
//! - 生成审计报告（因果链、版本历史）
//! - 验证审计链完整性
//!
//! # 哈希链算法
//! - `last_hash` 初始为 `"genesis"`
//! - 每个事实的链哈希 = `blake3(prev_hash + fact_hash)`
//! - 该链哈希作为下一条目的 `prev_hash`，形成不可篡改的链式结构

use crate::clock::LogicalClock;
use crate::hash;
use std::collections::BTreeMap;
use tier1_reactor::{Fact, FactId, FactsLog};

/// 审计条目
///
/// 对应一个被审计的 [`Fact`]，记录其哈希、逻辑时间戳及哈希链前驱。
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// Fact ID
    pub fact_id: FactId,
    /// Fact 类型名
    pub fact_type: &'static str,
    /// 逻辑时钟值
    pub logical_time: u64,
    /// 内容哈希
    pub content_hash: String,
    /// 前一条目的哈希（形成哈希链）
    pub prev_hash: String,
    /// 因果父 Fact ID（如有）
    pub cause: Option<FactId>,
}

/// 审计器
///
/// 周期性消费 [`FactsLog`] 中的新增事实，构建带哈希链的审计条目列表。
/// 维护 `BTreeMap<FactId, usize>` 实时索引，支持 O(log n) 的因果链查询。
///
/// # 持久化（WAL）
/// 可选地通过 [`Auditor::with_wal_path`] 启用 WAL 持久化。
/// 启用后，每个新增审计条目以 JSONL 格式追加写入磁盘，
/// 进程重启后可通过 [`Auditor::load_from_wal`] 恢复审计状态。
pub struct Auditor {
    /// FactsLog 引用
    facts_log: FactsLog,
    /// 逻辑时钟
    clock: LogicalClock,
    /// 已审计到的版本
    last_audited_version: u64,
    /// 审计条目列表
    entries: Vec<AuditEntry>,
    /// 上一条目的哈希
    last_hash: String,
    /// FactId -> entries 索引的实时索引（优化因果链查询）
    index: BTreeMap<FactId, usize>,
    /// WAL 持久化路径（可选）
    wal_path: Option<std::path::PathBuf>,
}

impl Auditor {
    /// 创建新审计器
    ///
    /// `last_hash` 初始为 `"genesis"`，`last_audited_version` 初始为 0。
    pub fn new(facts_log: FactsLog) -> Self {
        Self {
            facts_log,
            clock: LogicalClock::new(),
            last_audited_version: 0,
            entries: Vec::new(),
            last_hash: String::from("genesis"),
            index: BTreeMap::new(),
            wal_path: None,
        }
    }

    /// 设置 WAL 持久化路径
    ///
    /// 启用后，每次 `audit_new` 产生的新条目将以 JSONL 格式
    /// 追加写入该路径。进程重启后可通过 `load_from_wal` 恢复。
    pub fn with_wal_path<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.wal_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// 将单条审计条目序列化为 JSONL 行
    fn entry_to_json_line(entry: &AuditEntry) -> String {
        serde_json::json!({
            "fact_id": entry.fact_id.0,
            "fact_type": entry.fact_type,
            "logical_time": entry.logical_time,
            "content_hash": entry.content_hash,
            "prev_hash": entry.prev_hash,
            "cause": entry.cause.map(|c| c.0),
        })
        .to_string()
    }

    /// 追加单条审计条目到 WAL 文件
    fn append_wal(&self, entry: &AuditEntry) {
        if let Some(ref path) = self.wal_path {
            let line = Self::entry_to_json_line(entry);
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    /// 审计新增事实
    ///
    /// 从 FactsLog 读取 `last_audited_version` 之后的所有事实，
    /// 为每个事实创建 [`AuditEntry`]，更新哈希链。
    ///
    /// # 去重策略
    /// 由于 `FactsLog::read_from` 按 `version_before` 过滤，而同一版本下可能
    /// 存在多条不改变版本号的事实（如 `Command`/`IoRequest`），直接以版本号
    /// 做去重会导致重复审计。故本实现以 `entries.len()` 作为已审计进度，
    /// 从 `FactsLog::history()` 中读取尚未审计的尾部事实，保证每条事实仅审计一次。
    ///
    /// # 返回值
    /// 本次新增的审计条目数量。
    pub fn audit_new(&mut self) -> usize {
        let history = self.facts_log.history();
        let start = self.entries.len();
        if start >= history.len() {
            self.last_audited_version = self.facts_log.version();
            tracing::debug!(version = self.last_audited_version, "audit_new: 无新增事实");
            return 0;
        }

        let count = history.len() - start;
        for (idx_offset, fact) in history[start..].iter().enumerate() {
            let fact_id = fact.id();
            let fact_type = fact.type_name();
            let logical_time = self.clock.tick();
            let content_hash = hash::fact_hash(fact);
            let prev_hash = self.last_hash.clone();
            let cause = extract_cause(fact);

            // 计算新的链哈希：blake3(prev_hash + content_hash)
            let combined = format!("{}{}", prev_hash, content_hash);
            let new_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();
            self.last_hash = new_hash;

            let entry_index = start + idx_offset;
            self.index.insert(fact_id, entry_index);

            self.entries.push(AuditEntry {
                fact_id,
                fact_type,
                logical_time,
                content_hash,
                prev_hash,
                cause,
            });

            // 写入 WAL（如启用）
            self.append_wal(&self.entries[entry_index]);
        }

        self.last_audited_version = self.facts_log.version();
        tracing::debug!(
            audited = count,
            version = self.last_audited_version,
            "audit_new: 完成"
        );
        count
    }

    /// 验证审计链完整性
    ///
    /// 重新计算每个条目的链哈希，并校验其存储的 `prev_hash` 与上一条目
    /// 重算出的链哈希一致，确认链式结构未被篡改。
    ///
    /// # 返回值
    /// - `true`：所有条目的 `prev_hash` 链接自洽
    /// - `false`：发现断裂（某条目 `prev_hash` 与前驱重算哈希不匹配）
    pub fn verify(&self) -> bool {
        let mut prev_hash = String::from("genesis");
        for entry in &self.entries {
            if entry.prev_hash != prev_hash {
                tracing::warn!(
                    fact_id = entry.fact_id.0,
                    "verify: 哈希链断裂，prev_hash 不匹配"
                );
                return false;
            }
            // 重新计算当前条目的链哈希，作为下一跳的预期 prev_hash
            let combined = format!("{}{}", entry.prev_hash, entry.content_hash);
            let recomputed = blake3::hash(combined.as_bytes()).to_hex().to_string();
            prev_hash = recomputed;
        }
        tracing::debug!(entries = self.entries.len(), "verify: 审计链完整");
        true
    }

    /// 获取审计条目
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// 生成审计报告（JSON 格式）
    ///
    /// 包含审计器元信息（已审计版本、条目数、末尾哈希）与全部审计条目。
    pub fn report(&self) -> String {
        let entries_json: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "fact_id": e.fact_id.0,
                    "fact_type": e.fact_type,
                    "logical_time": e.logical_time,
                    "content_hash": e.content_hash,
                    "prev_hash": e.prev_hash,
                    "cause": e.cause.map(|c| c.0),
                })
            })
            .collect();

        let report = serde_json::json!({
            "last_audited_version": self.last_audited_version,
            "entry_count": self.entries.len(),
            "last_hash": self.last_hash,
            "entries": entries_json,
        });

        serde_json::to_string_pretty(&report).unwrap_or_else(|_| String::from("{}"))
    }

    /// 获取因果链（从指定 FactId 开始追溯 cause 链）
    ///
    /// 返回从该 FactId 起，沿 `cause` 字段向上追溯的审计条目列表，
    /// 顺序为：起始条目 → 其因果父 → … → 根因（`cause` 为 `None` 的条目）。
    /// 若中途找不到对应条目，则在断点处终止追溯。
    ///
    /// # 性能优化
    /// 使用实时维护的 `BTreeMap<FactId, usize>` 索引，查询复杂度为 O(log n)。
    pub fn causal_chain(&self, fact_id: FactId) -> Vec<AuditEntry> {
        let mut chain = Vec::new();
        let mut current = Some(fact_id);
        while let Some(cur_id) = current {
            match self.index.get(&cur_id) {
                Some(&i) => {
                    let entry = self.entries[i].clone();
                    current = entry.cause;
                    chain.push(entry);
                }
                None => {
                    tracing::warn!(fact_id = cur_id.0, "causal_chain: 追溯中断，未找到审计条目");
                    break;
                }
            }
        }
        chain
    }

    /// 从 WAL 文件加载审计状态
    ///
    /// 读取 WAL 文件中的 JSONL 行，重建 `entries`、`index`、`last_hash`
    /// 和 `clock`。文件不存在或为空时返回空状态。
    ///
    /// # 注意
    /// 此方法不校验 FactsLog 中是否存在对应 Fact，仅恢复内存结构。
    /// 调用方应确保 WAL 文件来自可信来源。
    pub fn load_from_wal(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::BufRead;

        if !path.exists() {
            return Ok(());
        }

        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let parsed: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => {
                    tracing::warn!(line = idx, "load_from_wal: 跳过无效 JSON 行");
                    continue;
                }
            };

            let fact_id_num = match parsed.get("fact_id").and_then(|v| v.as_u64()) {
                Some(n) => n,
                None => continue,
            };
            let fact_type = match parsed.get("fact_type").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let logical_time = parsed
                .get("logical_time")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let content_hash = parsed
                .get("content_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let prev_hash = parsed
                .get("prev_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cause = parsed.get("cause").and_then(|v| v.as_u64()).map(FactId);

            let fact_id = FactId(fact_id_num);
            let entry = AuditEntry {
                fact_id,
                // 安全：fact_type 是静态字符串名（来自 Fact::type_name），
                // 此处使用泄漏扩展生命周期。WAL 中的类型名都是已知的固定字符串，
                // 实际使用中应做类型映射表。这里做简化处理。
                fact_type: FACT_TYPE_STATIC_TABLE
                    .iter()
                    .copied()
                    .find(|t| *t == fact_type)
                    .unwrap_or("Unknown"),
                logical_time,
                content_hash: content_hash.clone(),
                prev_hash: prev_hash.clone(),
                cause,
            };

            self.index.insert(fact_id, self.entries.len());
            self.entries.push(entry);

            // 重算链哈希（验证并更新 last_hash）
            let combined = format!("{}{}", prev_hash, content_hash);
            self.last_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();

            // 更新时钟到最大值
            if logical_time > self.clock.current() {
                // 逻辑时钟通过 tick 推进；从 WAL 恢复时直接设置到最大值+1
                // 这里我们用一个小技巧：多次 tick 直到达到目标
                while self.clock.current() < logical_time {
                    self.clock.tick();
                }
            }
        }

        self.wal_path = Some(path.to_path_buf());
        Ok(())
    }
}

/// Fact 类型名静态表（用于 WAL 加载时获取 &'static str）
///
/// 包含所有已知的 Fact 变体类型名，加载 WAL 时从中查找匹配项，
/// 以获取 `&'static str` 引用。
const FACT_TYPE_STATIC_TABLE: &[&str] = &[
    "StateTransition",
    "Command",
    "PayloadUpdate",
    "IoRequest",
    "IoResponse",
    "ControlSignal",
    "Error",
    "Unknown",
];

/// 从 Fact 中提取因果父 Fact ID
///
/// - [`Fact::StateTransition`] / [`Fact::IoRequest`]：返回其 `cause` 字段
/// - [`Fact::IoResponse`]：返回其 `request_id`（语义上为因果前驱）
/// - 其他变体：返回 `None`
fn extract_cause(fact: &Fact) -> Option<FactId> {
    match fact {
        Fact::StateTransition { cause, .. } => Some(*cause),
        Fact::IoRequest { cause, .. } => Some(*cause),
        Fact::IoResponse { request_id, .. } => Some(*request_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tier0_tcb::JsonValue;
    use tier1_reactor::{Fact, FactId, FactsLog, IoType};

    fn make_facts_log() -> FactsLog {
        FactsLog::new()
    }

    #[test]
    fn test_auditor_basic_chain() {
        let log = make_facts_log();
        let f0 = Fact::PayloadUpdate {
            id: FactId(1),
            path: "k1".into(),
            value: JsonValue::string("v1"),
        };
        let id0 = f0.id();
        log.append(f0).unwrap();

        let f1 = Fact::StateTransition {
            id: FactId(2),
            cause: id0,
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };
        log.append(f1).unwrap();

        let mut auditor = Auditor::new(log);
        let n = auditor.audit_new();
        assert_eq!(n, 2);
        assert!(auditor.verify());
        assert_eq!(auditor.entries().len(), 2);
    }

    #[test]
    fn test_auditor_causal_chain() {
        let log = make_facts_log();

        let f0 = Fact::PayloadUpdate {
            id: FactId(1),
            path: "root".into(),
            value: JsonValue::from(0i64),
        };
        let id0 = f0.id();
        log.append(f0).unwrap();

        let f1 = Fact::StateTransition {
            id: FactId(2),
            cause: id0,
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };
        let id1 = f1.id();
        log.append(f1).unwrap();

        let f2 = Fact::IoRequest {
            id: FactId(3),
            cause: id1,
            io_type: IoType::HttpGet,
            params: JsonValue::empty_object(),
        };
        let id2 = f2.id();
        log.append(f2).unwrap();

        let mut auditor = Auditor::new(log);
        auditor.audit_new();

        let chain = auditor.causal_chain(id2);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].fact_id, id2);
        assert_eq!(chain[1].fact_id, id1);
        assert_eq!(chain[2].fact_id, id0);
    }

    #[test]
    fn test_auditor_wal_persist_and_reload() {
        let tmp =
            std::env::temp_dir().join(format!("auditor_wal_test_{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        // 第一轮：写入 WAL
        {
            let log = make_facts_log();
            let f0 = Fact::PayloadUpdate {
                id: FactId(1),
                path: "a".into(),
                value: JsonValue::from(1i64),
            };
            let f1 = Fact::PayloadUpdate {
                id: FactId(2),
                path: "b".into(),
                value: JsonValue::from(2i64),
            };
            log.append(f0).unwrap();
            log.append(f1).unwrap();

            let mut auditor = Auditor::new(log).with_wal_path(&tmp);
            let n = auditor.audit_new();
            assert_eq!(n, 2);
            assert!(auditor.verify());
        }

        // 第二轮：从 WAL 恢复
        {
            let log = make_facts_log();
            let mut auditor = Auditor::new(log);
            auditor.load_from_wal(&tmp).expect("load wal");
            assert_eq!(auditor.entries().len(), 2);
            assert!(auditor.verify());
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_auditor_wal_nonexistent_file() {
        let log = make_facts_log();
        let mut auditor = Auditor::new(log);
        let result = auditor.load_from_wal(std::path::Path::new("/nonexistent/path/wal.jsonl"));
        assert!(result.is_ok());
        assert_eq!(auditor.entries().len(), 0);
    }

    #[test]
    fn test_auditor_wal_incremental() {
        let tmp =
            std::env::temp_dir().join(format!("auditor_wal_incr_{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let log = make_facts_log();
        let mut auditor = Auditor::new(log.clone()).with_wal_path(&tmp);

        // 第一批
        let f1 = Fact::PayloadUpdate {
            id: FactId(1),
            path: "k1".into(),
            value: JsonValue::string("v1"),
        };
        log.append(f1).unwrap();
        assert_eq!(auditor.audit_new(), 1);

        // 第二批
        let f2 = Fact::PayloadUpdate {
            id: FactId(2),
            path: "k2".into(),
            value: JsonValue::string("v2"),
        };
        log.append(f2).unwrap();
        assert_eq!(auditor.audit_new(), 1);

        // 从 WAL 恢复应得到 2 条
        let mut auditor2 = Auditor::new(make_facts_log());
        auditor2.load_from_wal(&tmp).expect("load wal");
        assert_eq!(auditor2.entries().len(), 2);
        assert!(auditor2.verify());

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_auditor_empty() {
        let log = make_facts_log();
        let mut auditor = Auditor::new(log);
        assert_eq!(auditor.audit_new(), 0);
        assert!(auditor.verify());
        assert_eq!(auditor.entries().len(), 0);
    }

    #[test]
    fn test_auditor_incremental_idempotent() {
        let log = make_facts_log();
        let f = Fact::PayloadUpdate {
            id: FactId(1),
            path: "x".into(),
            value: JsonValue::string("y"),
        };
        log.append(f).unwrap();

        let mut auditor = Auditor::new(log);
        assert_eq!(auditor.audit_new(), 1);
        // 再次调用 audit_new，无新事实，应返回 0
        assert_eq!(auditor.audit_new(), 0);
        assert_eq!(auditor.audit_new(), 0);
        assert_eq!(auditor.entries().len(), 1);
    }

    #[test]
    fn test_auditor_causal_chain_not_found() {
        let log = make_facts_log();
        let f = Fact::PayloadUpdate {
            id: FactId(1),
            path: "x".into(),
            value: JsonValue::string("y"),
        };
        log.append(f).unwrap();

        let mut auditor = Auditor::new(log);
        auditor.audit_new();

        // 不存在的 fact_id
        let chain = auditor.causal_chain(FactId(999));
        assert!(chain.is_empty());
    }

    #[test]
    fn test_auditor_causal_chain_root() {
        let log = make_facts_log();
        let f = Fact::PayloadUpdate {
            id: FactId(1),
            path: "x".into(),
            value: JsonValue::string("y"),
        };
        let id = f.id();
        log.append(f).unwrap();

        let mut auditor = Auditor::new(log);
        auditor.audit_new();

        // 根因（没有 cause）的因果链只有自己
        let chain = auditor.causal_chain(id);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].fact_id, id);
        assert!(chain[0].cause.is_none());
    }

    #[test]
    fn test_auditor_report_format() {
        let log = make_facts_log();
        let f = Fact::PayloadUpdate {
            id: FactId(1),
            path: "k".into(),
            value: JsonValue::string("v"),
        };
        log.append(f).unwrap();

        let mut auditor = Auditor::new(log);
        auditor.audit_new();

        let report = auditor.report();
        let parsed: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["entry_count"], 1);
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 1);
        assert!(parsed["last_hash"].is_string());
        assert!(parsed["last_audited_version"].is_number());
    }

    #[test]
    fn test_auditor_verify_empty_chain() {
        let log = make_facts_log();
        let auditor = Auditor::new(log);
        assert!(auditor.verify());
    }

    #[test]
    fn test_auditor_io_response_causal_link() {
        let log = make_facts_log();
        let req = Fact::IoRequest {
            id: FactId(10),
            cause: FactId(1),
            io_type: IoType::HttpGet,
            params: JsonValue::empty_object(),
        };
        let req_id = req.id();
        log.append(req).unwrap();

        let resp = Fact::IoResponse {
            id: FactId(11),
            request_id: req_id,
            result: JsonValue::string("ok"),
            error: None,
        };
        let resp_id = resp.id();
        log.append(resp).unwrap();

        let mut auditor = Auditor::new(log);
        auditor.audit_new();

        // IoResponse 的因果父是 request_id
        let chain = auditor.causal_chain(resp_id);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fact_id, resp_id);
        assert_eq!(chain[1].fact_id, req_id);
    }

    #[test]
    fn test_auditor_last_hash_genesis() {
        let log = make_facts_log();
        let auditor = Auditor::new(log);
        // 初始 last_hash 为 "genesis"
        // 通过 report 间接验证
        let report = auditor.report();
        let parsed: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["last_hash"], "genesis");
    }

    #[test]
    fn test_auditor_wal_with_invalid_lines() {
        let tmp =
            std::env::temp_dir().join(format!("auditor_wal_bad_{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        // 手动写入混合有效和无效行的 WAL
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).unwrap();
        writeln!(file, "not valid json").unwrap();
        writeln!(
            file,
            r#"{{"fact_id": 5, "fact_type": "PayloadUpdate", "logical_time": 3, "content_hash": "abc", "prev_hash": "genesis", "cause": null}}"#
        )
        .unwrap();
        writeln!(file, "").unwrap(); // 空行
        drop(file);

        let log = make_facts_log();
        let mut auditor = Auditor::new(log);
        auditor.load_from_wal(&tmp).expect("load wal");

        // 只有 1 条有效条目
        assert_eq!(auditor.entries().len(), 1);
        assert_eq!(auditor.entries()[0].fact_id, FactId(5));
        assert_eq!(auditor.entries()[0].fact_type, "PayloadUpdate");

        let _ = std::fs::remove_file(&tmp);
    }
}
