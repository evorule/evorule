#![forbid(unsafe_code)]
//! 时间机器 —— rewind / fork / diff / replay API（阶段5，软回滚模式）
//!
//! # 设计依据
//!
//! 文档 14 §三 第三组：时间机器（rewind/fork/diff/replay）。
//! 文档 14 §八 决策点4：采用**软回滚模式**（fork 语义）——
//! - rewind 是只读操作：从 FactsLog 重放到指定 version，返回当时的快照
//! - 真正的"回到过去执行"用 fork：创建新 Reactor 实例，独立 fact 流
//! - 原 reactor 继续运行，两份 facts 都保留（不丢弃历史）
//!
//! # 规范合规（§2.3）
//!
//! - ✅ TCB 是纯函数，replay 确定
//! - ✅ core_eval 启动时定，replay 跨 version 一致
//! - ✅ FactsLog append-only，rewind 不破坏因果
//! - ✅ fork = 新 reactor 实例（不破坏 TCB 纯函数假设）
//! - ✅ 数据加载与结构转换是机制（Rust 可写，见 §2.1）

use crate::fact::Fact;
use crate::facts_log::FactsLog;
use crate::reactor::Reactor;
use tier0_tcb::JsonValue;

/// rewind 结果：指定 version 的物化快照
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindSnapshot {
    /// 该 version 的 payload
    pub payload: JsonValue,
    /// 该 version 的指令队列
    pub queue: Vec<JsonValue>,
    /// 实际到达的 version
    pub version: u64,
}

/// 两个 version 间的 payload diff 结果
///
/// 比较两个 payload 的**顶层字段**（不递归嵌套对象）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadDiff {
    /// v_b 新增的字段（v_a 没有，v_b 有）
    pub added: Vec<(String, JsonValue)>,
    /// v_b 删除的字段（v_a 有，v_b 没有）
    pub removed: Vec<(String, JsonValue)>,
    /// 值变化的字段 (key, v_a 的值, v_b 的值)
    pub changed: Vec<(String, JsonValue, JsonValue)>,
    /// 值未变化的字段名
    pub unchanged: Vec<String>,
}

impl PayloadDiff {
    /// diff 是否为空（两个 payload 完全相同）
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// 变更字段总数（added + removed + changed）
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    /// 生成可读的 diff 摘要
    pub fn summary(&self) -> String {
        format!(
            "diff: +{} -{} ~{} (={} unchanged)",
            self.added.len(),
            self.removed.len(),
            self.changed.len(),
            self.unchanged.len()
        )
    }
}

/// 软回滚：从 FactsLog 重放到指定 version，返回当时的快照
///
/// **软回滚模式**：只读操作，不修改当前 reactor 状态，不丢弃 facts。
/// 真正的分叉执行用 [`fork`]。
///
/// # 重放规则
///
/// - `StateTransition`：更新 payload 和 queue，version += 1
/// - `IoResponse`：version += 1（payload 由后续 StateTransition 更新）
/// - `Stable`：不改变 version
/// - 其他 Fact（Command/PayloadUpdate/IoRequest/Error）：不改变 version
///
/// # 参数
///
/// - `facts_log`: 审计链引用
/// - `target_version`: 目标版本号（0 = 初始空状态）
///
/// # 返回
///
/// - `Some(RewindSnapshot)`: 成功重放到 target_version
/// - `None`: target_version 超出当前最大版本
pub fn rewind(facts_log: &FactsLog, target_version: u64) -> Option<RewindSnapshot> {
    // version 0 = 初始空状态
    if target_version == 0 {
        return Some(RewindSnapshot {
            payload: JsonValue::empty_object(),
            queue: Vec::new(),
            version: 0,
        });
    }

    let history = facts_log.history_with_versions();
    let mut payload = JsonValue::empty_object();
    let mut queue: Vec<JsonValue> = Vec::new();
    let mut version: u64 = 0;

    for (version_before, fact) in history {
        match &fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => {
                payload = new_payload.clone();
                queue = new_queue.clone();
                version = version_before + 1;
            }
            Fact::IoResponse { .. } => {
                version = version_before + 1;
            }
            _ => {}
        }
        if version == target_version {
            break;
        }
    }

    if version < target_version {
        // 目标版本超出当前最大版本
        return None;
    }

    Some(RewindSnapshot {
        payload,
        queue,
        version,
    })
}

/// 从指定 version 分叉：创建新 Reactor 实例，独立 fact 流
///
/// **软回滚模式**：原 reactor 继续运行，两份 facts 都保留。
/// 新 reactor 从 `from_version` 的 payload 开始，queue 为空（新分支从 clean state 开始），
/// 拥有独立的 FactsLog（不含原 reactor 的历史）。
///
/// # 参数
///
/// - `facts_log`: 原 reactor 的审计链
/// - `from_version`: 分叉点版本号
/// - `core_eval`: 新 reactor 的 core_eval 规则（通常与原 reactor 相同）
/// - `max_rounds`: 新 reactor 的最大指令执行步数
///
/// # 返回
///
/// - `Some(Reactor)`: 已配置初始 payload 的 reactor（调用者负责 spawn）
/// - `None`: from_version 超出当前最大版本
///
/// # 使用示例
///
/// ```ignore
/// let new_reactor = time_machine::fork(&facts_log, 5, core_eval.clone(), 10000)?;
/// let (tx, rx, evt_tx, handle, new_facts_log) = new_reactor.spawn();
/// ```
pub fn fork(
    facts_log: &FactsLog,
    from_version: u64,
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
) -> Option<Reactor> {
    let snapshot = rewind(facts_log, from_version)?;

    // 先提取 payload keys 数量（payload 将被 move 到 builder）
    let payload_keys = snapshot.payload.as_object().map(|m| m.len()).unwrap_or(0);

    let reactor = Reactor::builder(core_eval)
        .max_rounds(max_rounds)
        .initial_payload(snapshot.payload)
        .build();

    tracing::info!(
        fork_from_version = from_version,
        fork_payload_keys = payload_keys,
        "Forked new reactor from version (soft-rollback mode)"
    );

    Some(reactor)
}

/// 两个 version 间的 payload diff
///
/// 比较两个 version 的 payload **顶层字段**：
/// - `added`: v_b 新增的字段
/// - `removed`: v_b 删除的字段
/// - `changed`: 值变化的字段
/// - `unchanged`: 值相同的字段名
///
/// 若 v_a 或 v_b 超出当前最大版本，按空对象处理。
pub fn diff(facts_log: &FactsLog, v_a: u64, v_b: u64) -> PayloadDiff {
    let payload_a = rewind(facts_log, v_a)
        .map(|s| s.payload)
        .unwrap_or_else(JsonValue::empty_object);
    let payload_b = rewind(facts_log, v_b)
        .map(|s| s.payload)
        .unwrap_or_else(JsonValue::empty_object);

    compute_diff(&payload_a, &payload_b)
}

/// 返回 (from_v, to_v] 间的所有 facts
///
/// 基于 `version_before` 过滤：返回 `from_v < version_before <= to_v` 的所有 Fact。
///
/// # 参数
///
/// - `from_v`: 起始版本（排除）
/// - `to_v`: 结束版本（包含）
///
/// # 注意
///
/// - `from_v >= to_v` 时返回空
/// - `to_v` 超出当前最大版本时，返回到实际最大版本为止
pub fn replay(facts_log: &FactsLog, from_v: u64, to_v: u64) -> Vec<Fact> {
    if from_v >= to_v {
        return Vec::new();
    }

    let history = facts_log.history_with_versions();
    history
        .into_iter()
        .filter(|(version_before, _)| *version_before > from_v && *version_before <= to_v)
        .map(|(_, fact)| fact)
        .collect()
}

/// 计算两个 payload（JsonValue::Object）的顶层字段 diff
fn compute_diff(payload_a: &JsonValue, payload_b: &JsonValue) -> PayloadDiff {
    let obj_a = payload_a.as_object();
    let obj_b = payload_b.as_object();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();

    // 遍历 v_a 的字段：检查 removed / changed / unchanged
    if let Some(map_a) = obj_a {
        for (key, val_a) in map_a {
            match obj_b.and_then(|m| m.get(key)) {
                Some(val_b) => {
                    if val_a == val_b {
                        unchanged.push(key.clone());
                    } else {
                        changed.push((key.clone(), val_a.clone(), val_b.clone()));
                    }
                }
                None => {
                    removed.push((key.clone(), val_a.clone()));
                }
            }
        }
    }

    // 遍历 v_b 的字段：检查 added（v_a 中不存在的字段）
    if let Some(map_b) = obj_b {
        for (key, val_b) in map_b {
            if obj_a.and_then(|m| m.get(key)).is_none() {
                added.push((key.clone(), val_b.clone()));
            }
        }
    }

    PayloadDiff {
        added,
        removed,
        changed,
        unchanged,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::FactId;

    /// 辅助：构造 StateTransition fact
    fn st(id: u64, cause: u64, payload: JsonValue, queue: Vec<JsonValue>) -> Fact {
        Fact::StateTransition {
            id: FactId(id),
            cause: FactId(cause),
            new_payload: payload,
            new_queue: queue,
        }
    }

    /// 辅助：构造 Command fact
    fn cmd(id: u64) -> Fact {
        Fact::Command {
            id: FactId(id),
            instruction: JsonValue::empty_object(),
        }
    }

    /// 辅助：构造 IoResponse fact
    fn io_resp(id: u64, req_id: u64) -> Fact {
        Fact::IoResponse {
            id: FactId(id),
            request_id: FactId(req_id),
            result: JsonValue::string("ok"),
            error: None,
        }
    }

    /// 辅助：构造 Stable fact
    fn stable(id: u64, snap: JsonValue) -> Fact {
        Fact::Stable {
            id: FactId(id),
            final_snapshot: snap,
        }
    }

    // ===== rewind 测试 =====

    #[test]
    fn test_rewind_version_zero_returns_empty() {
        let log = FactsLog::new();
        let snap = rewind(&log, 0).unwrap();
        assert_eq!(snap.version, 0);
        assert_eq!(snap.payload, JsonValue::empty_object());
        assert!(snap.queue.is_empty());
    }

    #[test]
    fn test_rewind_to_nonexistent_version_returns_none() {
        let log = FactsLog::new();
        log.append(cmd(1)).unwrap();
        // version 仍为 0（Command 不增版本），rewind(1) 应返回 None
        assert!(rewind(&log, 1).is_none());
    }

    #[test]
    fn test_rewind_after_state_transition() {
        let log = FactsLog::new();
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]);
        log.append(st(1, 0, payload.clone(), vec![])).unwrap();
        // version 现在为 1
        let snap = rewind(&log, 1).unwrap();
        assert_eq!(snap.version, 1);
        assert_eq!(snap.payload, payload);
        assert!(snap.queue.is_empty());
    }

    #[test]
    fn test_rewind_preserves_queue() {
        let log = FactsLog::new();
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]);
        let queue = vec![JsonValue::string("instr1"), JsonValue::string("instr2")];
        log.append(st(1, 0, payload.clone(), queue.clone()))
            .unwrap();
        let snap = rewind(&log, 1).unwrap();
        assert_eq!(snap.queue, queue);
    }

    #[test]
    fn test_rewind_to_intermediate_version() {
        let log = FactsLog::new();
        // v0 → v1: payload {x:1}
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        // v1 → v2: payload {x:2}
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(2))]),
            vec![],
        ))
        .unwrap();
        // v2 → v3: payload {x:3}
        log.append(st(
            3,
            2,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(3))]),
            vec![],
        ))
        .unwrap();

        // rewind 到 v1：应为 {x:1}
        let snap1 = rewind(&log, 1).unwrap();
        assert_eq!(snap1.version, 1);
        assert_eq!(snap1.payload.get("x"), Some(&JsonValue::Integer(1)));

        // rewind 到 v2：应为 {x:2}
        let snap2 = rewind(&log, 2).unwrap();
        assert_eq!(snap2.version, 2);
        assert_eq!(snap2.payload.get("x"), Some(&JsonValue::Integer(2)));
    }

    #[test]
    fn test_rewind_skips_non_version_facts() {
        let log = FactsLog::new();
        // Command（不增版本）
        log.append(cmd(1)).unwrap();
        // v0 → v1: StateTransition
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            vec![],
        ))
        .unwrap();
        // Stable（不增版本）
        log.append(stable(2, JsonValue::empty_object())).unwrap();

        let snap = rewind(&log, 1).unwrap();
        assert_eq!(snap.version, 1);
        assert_eq!(snap.payload.get("x"), Some(&JsonValue::Integer(42)));
    }

    #[test]
    fn test_rewind_io_response_increments_version() {
        let log = FactsLog::new();
        // v0 → v1: StateTransition
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        // v1 → v2: IoResponse
        log.append(io_resp(2, 1)).unwrap();

        // rewind 到 v2：payload 仍为 StateTransition 的值
        let snap = rewind(&log, 2).unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.payload.get("x"), Some(&JsonValue::Integer(1)));
    }

    #[test]
    fn test_rewind_does_not_modify_facts_log() {
        let log = FactsLog::new();
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            vec![],
        ))
        .unwrap();
        let len_before = log.history_len();
        let _ = rewind(&log, 1);
        let len_after = log.history_len();
        // 软回滚：不修改 facts_log
        assert_eq!(len_before, len_after);
    }

    // ===== diff 测试 =====

    #[test]
    fn test_diff_identical_payloads() {
        let log = FactsLog::new();
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        let diff_result = diff(&log, 1, 1);
        assert!(diff_result.is_empty());
        assert_eq!(diff_result.change_count(), 0);
        assert_eq!(diff_result.unchanged.len(), 1);
    }

    #[test]
    fn test_diff_added_field() {
        let log = FactsLog::new();
        // v1: {x:1}
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        // v2: {x:1, y:2}
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(1)),
                ("y", JsonValue::Integer(2)),
            ]),
            vec![],
        ))
        .unwrap();

        let diff_result = diff(&log, 1, 2);
        assert_eq!(diff_result.added.len(), 1);
        assert_eq!(diff_result.added[0].0, "y");
        assert_eq!(diff_result.removed.len(), 0);
        assert_eq!(diff_result.changed.len(), 0);
        assert_eq!(diff_result.unchanged.len(), 1); // x
    }

    #[test]
    fn test_diff_removed_field() {
        let log = FactsLog::new();
        // v1: {x:1, y:2}
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(1)),
                ("y", JsonValue::Integer(2)),
            ]),
            vec![],
        ))
        .unwrap();
        // v2: {x:1}
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();

        let diff_result = diff(&log, 1, 2);
        assert_eq!(diff_result.removed.len(), 1);
        assert_eq!(diff_result.removed[0].0, "y");
        assert_eq!(diff_result.added.len(), 0);
        assert_eq!(diff_result.changed.len(), 0);
    }

    #[test]
    fn test_diff_changed_field() {
        let log = FactsLog::new();
        // v1: {x:1}
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        // v2: {x:2}
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(2))]),
            vec![],
        ))
        .unwrap();

        let diff_result = diff(&log, 1, 2);
        assert_eq!(diff_result.changed.len(), 1);
        assert_eq!(diff_result.changed[0].0, "x");
        assert_eq!(diff_result.changed[0].1, JsonValue::Integer(1));
        assert_eq!(diff_result.changed[0].2, JsonValue::Integer(2));
        assert_eq!(diff_result.added.len(), 0);
        assert_eq!(diff_result.removed.len(), 0);
    }

    #[test]
    fn test_diff_mixed_changes() {
        let log = FactsLog::new();
        // v1: {x:1, a:10}
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(1)),
                ("a", JsonValue::Integer(10)),
            ]),
            vec![],
        ))
        .unwrap();
        // v2: {x:2, b:20}
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(2)),
                ("b", JsonValue::Integer(20)),
            ]),
            vec![],
        ))
        .unwrap();

        let diff_result = diff(&log, 1, 2);
        // x: 1→2 (changed)
        assert_eq!(diff_result.changed.len(), 1);
        // a: removed
        assert_eq!(diff_result.removed.len(), 1);
        assert_eq!(diff_result.removed[0].0, "a");
        // b: added
        assert_eq!(diff_result.added.len(), 1);
        assert_eq!(diff_result.added[0].0, "b");
        assert_eq!(diff_result.change_count(), 3);
    }

    #[test]
    fn test_diff_nonexistent_version_treated_as_empty() {
        let log = FactsLog::new();
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        // v1 存在，v5 不存在
        let diff_result = diff(&log, 1, 5);
        // v1 → 空：x 被 removed
        assert_eq!(diff_result.removed.len(), 1);
        assert_eq!(diff_result.added.len(), 0);
    }

    #[test]
    fn test_diff_summary_format() {
        let diff_result = PayloadDiff {
            added: vec![("a".to_string(), JsonValue::Integer(1))],
            removed: vec![("b".to_string(), JsonValue::Integer(2))],
            changed: vec![(
                "c".to_string(),
                JsonValue::Integer(3),
                JsonValue::Integer(4),
            )],
            unchanged: vec!["d".to_string()],
        };
        let summary = diff_result.summary();
        assert!(summary.contains("+1"));
        assert!(summary.contains("-1"));
        assert!(summary.contains("~1"));
        assert!(summary.contains("=1"));
    }

    // ===== replay 测试 =====

    #[test]
    fn test_replay_empty_range() {
        let log = FactsLog::new();
        log.append(cmd(1)).unwrap();
        // from_v >= to_v → 空
        assert!(replay(&log, 1, 1).is_empty());
        assert!(replay(&log, 2, 1).is_empty());
    }

    #[test]
    fn test_replay_full_range() {
        let log = FactsLog::new();
        log.append(cmd(1)).unwrap(); // version_before=0
        log.append(st(
            2,
            1,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap(); // version_before=0
        log.append(stable(3, JsonValue::empty_object())).unwrap(); // version_before=1

        // replay(0, 1)：version_before in (0, 1] → version_before=1
        let facts = replay(&log, 0, 1);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].id(), FactId(3)); // Stable
    }

    #[test]
    fn test_replay_partial_range() {
        let log = FactsLog::new();
        // v0→v1: StateTransition(id=1, version_before=0)
        log.append(st(1, 0, JsonValue::empty_object(), vec![]))
            .unwrap();
        // v1→v2: StateTransition(id=2, version_before=1)
        log.append(st(2, 1, JsonValue::empty_object(), vec![]))
            .unwrap();
        // v2→v3: StateTransition(id=3, version_before=2)
        log.append(st(3, 2, JsonValue::empty_object(), vec![]))
            .unwrap();

        // replay(0, 2)：version_before in (0, 2] → version_before=1 和 2
        // 对应 FactId(2) 和 FactId(3)（FactId(1) 的 version_before=0 不满足 > 0）
        let facts = replay(&log, 0, 2);
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].id(), FactId(2));
        assert_eq!(facts[1].id(), FactId(3));
    }

    #[test]
    fn test_replay_filters_by_version_before() {
        let log = FactsLog::new();
        // Command（version_before=0，不增版本）
        log.append(cmd(1)).unwrap();
        // v0→v1: StateTransition（version_before=0）
        log.append(st(2, 1, JsonValue::empty_object(), vec![]))
            .unwrap();
        // v1→v2: IoResponse（version_before=1）
        log.append(io_resp(3, 2)).unwrap();

        // replay(0, 1)：version_before in (0, 1] → 只有 version_before=1 的 IoResponse
        let facts = replay(&log, 0, 1);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].id(), FactId(3)); // IoResponse
    }

    #[test]
    fn test_replay_beyond_max_version() {
        let log = FactsLog::new();
        // v0→v1: StateTransition(id=1, version_before=0)
        log.append(st(1, 0, JsonValue::empty_object(), vec![]))
            .unwrap();
        // v1→v2: StateTransition(id=2, version_before=1)
        log.append(st(2, 1, JsonValue::empty_object(), vec![]))
            .unwrap();

        // to_v=100 超出最大版本（当前为 2），返回到实际最大版本为止
        // 过滤条件 version_before > 0 && <= 100 → 只有 version_before=1 的 FactId(2)
        let facts = replay(&log, 0, 100);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].id(), FactId(2));
    }

    // ===== fork 测试 =====

    #[test]
    fn test_fork_returns_reactor_with_initial_payload() {
        let log = FactsLog::new();
        let payload = JsonValue::object_from_pairs(&[
            ("x", JsonValue::Integer(42)),
            ("y", JsonValue::string("hello")),
        ]);
        log.append(st(1, 0, payload, vec![])).unwrap();

        let core_eval = vec![JsonValue::empty_object()];
        let reactor = fork(&log, 1, core_eval, 10000);
        assert!(reactor.is_some());
    }

    #[test]
    fn test_fork_nonexistent_version_returns_none() {
        let log = FactsLog::new();
        log.append(cmd(1)).unwrap(); // version 仍为 0

        let core_eval = vec![JsonValue::empty_object()];
        let reactor = fork(&log, 5, core_eval, 10000);
        assert!(reactor.is_none());
    }

    #[test]
    fn test_fork_does_not_modify_original_facts_log() {
        let log = FactsLog::new();
        log.append(st(
            1,
            0,
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
            vec![],
        ))
        .unwrap();
        let len_before = log.history_len();

        let core_eval = vec![JsonValue::empty_object()];
        let _reactor = fork(&log, 1, core_eval, 10000);

        // 软回滚：原 facts_log 不被修改
        assert_eq!(log.history_len(), len_before);
    }

    // ===== compute_diff 单元测试 =====

    #[test]
    fn test_compute_diff_both_empty() {
        let a = JsonValue::empty_object();
        let b = JsonValue::empty_object();
        let diff_result = compute_diff(&a, &b);
        assert!(diff_result.is_empty());
    }

    #[test]
    fn test_compute_diff_non_object_treated_as_empty() {
        // 非 Object 的 JsonValue 按 empty 处理
        let a = JsonValue::Integer(42);
        let b = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]);
        let diff_result = compute_diff(&a, &b);
        // a 视为空，b 的 x 是 added
        assert_eq!(diff_result.added.len(), 1);
        assert_eq!(diff_result.removed.len(), 0);
    }

    #[test]
    fn test_compute_diff_nested_objects_compared_by_equality() {
        // 嵌套对象按整体相等性比较（不递归 diff）
        let nested_a = JsonValue::object_from_pairs(&[("inner", JsonValue::Integer(1))]);
        let nested_b = JsonValue::object_from_pairs(&[("inner", JsonValue::Integer(2))]);
        let a = JsonValue::object_from_pairs(&[("obj", nested_a.clone())]);
        let b = JsonValue::object_from_pairs(&[("obj", nested_b.clone())]);

        let diff_result = compute_diff(&a, &b);
        // obj 值不同 → changed
        assert_eq!(diff_result.changed.len(), 1);
        assert_eq!(diff_result.changed[0].0, "obj");
    }

    // ===== PayloadDiff 方法测试 =====

    #[test]
    fn test_payload_diff_is_empty() {
        let empty = PayloadDiff {
            added: vec![],
            removed: vec![],
            changed: vec![],
            unchanged: vec!["x".to_string()],
        };
        assert!(empty.is_empty());

        let non_empty = PayloadDiff {
            added: vec![("y".to_string(), JsonValue::Integer(1))],
            removed: vec![],
            changed: vec![],
            unchanged: vec!["x".to_string()],
        };
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_payload_diff_change_count() {
        let diff_result = PayloadDiff {
            added: vec![("a".to_string(), JsonValue::Integer(1))],
            removed: vec![("b".to_string(), JsonValue::Integer(2))],
            changed: vec![(
                "c".to_string(),
                JsonValue::Integer(3),
                JsonValue::Integer(4),
            )],
            unchanged: vec![],
        };
        assert_eq!(diff_result.change_count(), 3);
    }
}
