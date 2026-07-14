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
        for fact in &history[start..] {
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

            self.entries.push(AuditEntry {
                fact_id,
                fact_type,
                logical_time,
                content_hash,
                prev_hash,
                cause,
            });
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
    pub fn causal_chain(&self, fact_id: FactId) -> Vec<AuditEntry> {
        // 建立 fact_id -> index 索引
        let mut index: BTreeMap<u64, usize> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            index.insert(e.fact_id.0, i);
        }

        let mut chain = Vec::new();
        let mut current = Some(fact_id);
        while let Some(cur_id) = current {
            match index.get(&cur_id.0) {
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
}

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
