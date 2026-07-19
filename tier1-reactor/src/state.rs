// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 反应器内部状态

use crate::fact::{FactId, IoType};
use crate::phase::ReactorPhase;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use tier0_tcb::JsonValue;

/// 反应器内部状态
#[derive(Debug, Clone)]
#[allow(dead_code)] // 部分工具方法供 tier2-governance 或扩展使用
pub(crate) struct ReactorState {
    /// 当前业务状态
    pub payload: JsonValue,

    /// 当前指令队列
    pub queue: VecDeque<JsonValue>,

    /// 单调递增版本号（每次状态变更 +1）
    pub version: u64,

    /// 上一次的版本号（用于不变式 #3：version 单调递增自检）
    ///
    /// 由 `bump_version` 维护：`prev_version = version; version += 1;`
    pub prev_version: u64,

    /// 待响应的 I/O 请求数量
    pub pending_io_count: usize,

    /// 待处理的 I/O 请求集合（用于验证 IoResponse）
    ///
    /// 使用 BTreeSet 保证确定性迭代顺序（与 tier0-tcb 风格一致）。
    pub pending_requests: BTreeSet<FactId>,

    /// 触发 I/O 的原指令缓存（IoRequest id → 原指令）
    ///
    /// IoResponse 到达后，反应器将原指令重新推送回队列前端，
    /// 使 core_eval.json 中的 `exists(__io_result__)` 双路径生效：
    /// - 首次执行：走 on_false 分支，触发 io_request
    /// - 恢复执行：走 on_true 分支，set 消费 __io_result__ 到业务字段
    pub pending_io_instructions: BTreeMap<FactId, JsonValue>,

    /// I/O 请求发射时间戳（P3-11：用于超时检测）
    ///
    /// 每次 `register_io_request` 时记录当前时间，
    /// 超时检测时扫描此映射判断是否超过 warn/error 阈值。
    pub pending_io_timestamps: BTreeMap<FactId, Instant>,

    /// I/O 请求类型记录（阶段3-1.4：用于按 io_type 查超时阈值）
    ///
    /// 每次 `register_io_request` 时记录 io_type，
    /// `check_io_timeouts` 时按 io_type 查 `IoTimeoutPolicy` 获取专属阈值。
    pub pending_io_types: BTreeMap<FactId, IoType>,

    /// I/O 恢复执行标志
    ///
    /// IoResponse 到达后设为 true，重新执行原指令后（execute_transition 返回 State）
    /// 反应器检查此标志，若为 true 则清除 `payload.__io_result__` 并重置为 false。
    /// 这是必要的，因为 `exists` 域检查的是"路径存在"，Null 值也算存在；
    /// 若不清除，后续不同的 I/O 指令会错误地走 on_true 分支（消费残留的旧结果）。
    pub io_recovery: bool,

    /// 控制层阶段（白盒化：让执行状态机可观察）
    ///
    /// 由主循环每次迭代更新，用于 tracing 与调试。
    /// 阶段转移函数是纯算法（见 [crate::phase::ReactorPhase::next]）。
    pub phase: ReactorPhase,

    /// 不变式违规累计计数（白盒化：结构性自检结果）
    ///
    /// 每次 phase 转移时调用 `check_invariants`，违规时累加。
    /// 仅记录，不强制中断反应器（用 `tracing::error!` 而非 `debug_assert!`，符合 F11）。
    pub invariant_violations: u64,
}

#[allow(dead_code)] // 部分队列操作方法供扩展使用
impl ReactorState {
    pub fn new() -> Self {
        Self {
            payload: JsonValue::empty_object(),
            queue: VecDeque::new(),
            version: 0,
            prev_version: 0,
            pending_io_count: 0,
            pending_requests: BTreeSet::new(),
            pending_io_instructions: BTreeMap::new(),
            pending_io_timestamps: BTreeMap::new(),
            pending_io_types: BTreeMap::new(),
            io_recovery: false,
            phase: ReactorPhase::default(),
            invariant_violations: 0,
        }
    }

    /// 安全递增版本号（同时更新 prev_version，支持不变式 #3 自检）
    ///
    /// 替代 `state.version += 1;`，确保 prev_version 同步更新。
    /// 在 PayloadUpdate / IoResponse / StateTransition 处理时调用。
    pub fn bump_version(&mut self) {
        self.prev_version = self.version;
        self.version += 1;
    }

    /// 稳定条件：队列空 + 无待处理 I/O
    pub fn is_stable(&self) -> bool {
        self.queue.is_empty() && self.pending_io_count == 0
    }

    /// 返回队列长度
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn pop_instruction(&mut self) -> Option<JsonValue> {
        self.queue.pop_front()
    }

    pub fn push_front(&mut self, instruction: JsonValue) {
        self.queue.push_front(instruction);
    }

    pub fn push_front_all(&mut self, instructions: Vec<JsonValue>) {
        // 反序后 push_front 保持原序：[a,b,c] → push c,b,a → 队列 [a,b,c,...]
        for instr in instructions.into_iter().rev() {
            self.queue.push_front(instr);
        }
    }

    pub fn push_back(&mut self, instruction: JsonValue) {
        self.queue.push_back(instruction);
    }

    pub fn push_back_all(&mut self, instructions: Vec<JsonValue>) {
        for instr in instructions {
            self.queue.push_back(instr);
        }
    }

    /// 注册一个待处理的 I/O 请求
    ///
    /// 幂等：重复注册同一 id 不会增加计数（防止 count 与 set 大小不一致）。
    /// P3-11：同时记录发射时间戳，用于超时检测。
    /// 阶段3-1.4：同时记录 io_type，用于按类型查超时阈值。
    pub fn register_io_request(&mut self, id: FactId, io_type: IoType) {
        if self.pending_requests.insert(id) {
            self.pending_io_count += 1;
            self.pending_io_timestamps.insert(id, Instant::now());
            self.pending_io_types.insert(id, io_type);
        }
    }

    /// 完成一个 I/O 请求，返回是否成功（即该请求是否在等待中）
    ///
    /// P3-11：同时移除时间戳。
    /// 阶段3-1.4：同时移除 io_type 记录。
    pub fn complete_io_request(&mut self, id: FactId) -> bool {
        if self.pending_requests.remove(&id) {
            self.pending_io_count = self.pending_io_count.saturating_sub(1);
            self.pending_io_timestamps.remove(&id);
            self.pending_io_types.remove(&id);
            true
        } else {
            false
        }
    }

    /// 扫描 pending I/O 超时（P3-11）
    ///
    /// 返回 `(warn_ids, error_ids)`：
    /// - `warn_ids`：超过 `warn_timeout` 但未超过 `error_timeout` 的请求 ID
    /// - `error_ids`：超过 `error_timeout` 的请求 ID（调用方应发射 Error 并恢复）
    pub fn scan_io_timeouts(
        &self,
        warn_timeout: Duration,
        error_timeout: Duration,
    ) -> (Vec<FactId>, Vec<FactId>) {
        let now = Instant::now();
        let mut warn_ids = Vec::new();
        let mut error_ids = Vec::new();
        for (id, timestamp) in &self.pending_io_timestamps {
            let elapsed = now.duration_since(*timestamp);
            if elapsed >= error_timeout {
                error_ids.push(*id);
            } else if elapsed >= warn_timeout {
                warn_ids.push(*id);
            }
        }
        (warn_ids, error_ids)
    }

    /// 强制移除一个超时的 I/O 请求（P3-11：error 超时恢复用）
    ///
    /// 与 `complete_io_request` 不同，此方法不期望 IoResponse 到达，
    /// 而是反应器主动清理超时请求。同时移除缓存的指令、时间戳和 io_type 记录。
    pub fn force_remove_io_request(&mut self, id: FactId) {
        self.pending_requests.remove(&id);
        self.pending_io_count = self.pending_io_count.saturating_sub(1);
        self.pending_io_instructions.remove(&id);
        self.pending_io_timestamps.remove(&id);
        self.pending_io_types.remove(&id);
    }

    /// 缓存触发 I/O 的原指令（IoRequest 产生时调用）
    ///
    /// IoResponse 到达后，通过 `take_io_instruction` 取出并重新推送回队列前端，
    /// 使 core_eval.json 中的双路径机制生效。
    pub fn save_io_instruction(&mut self, id: FactId, instruction: JsonValue) {
        self.pending_io_instructions.insert(id, instruction);
    }

    /// 取出并移除缓存的原指令（IoResponse 处理时调用）
    ///
    /// 返回 `Some(instruction)` 表示该 id 有缓存指令，应重新推送回队列；
    /// 返回 `None` 表示该 id 无缓存（可能是未知 IoResponse 或重复处理）。
    pub fn take_io_instruction(&mut self, id: FactId) -> Option<JsonValue> {
        self.pending_io_instructions.remove(&id)
    }

    /// 清除 payload 中的 `__io_result__` 字段
    ///
    /// I/O 恢复执行后调用，防止残留的 `__io_result__` 影响后续不同的 I/O 指令。
    /// 因为 `exists` 域检查的是"路径存在"（Null 也算存在），若不清除，
    /// 后续 I/O 指令会错误地走 on_true 分支，消费旧的 I/O 结果。
    pub fn clear_io_result(&mut self) {
        if let JsonValue::Object(map) = &mut self.payload {
            map.remove("__io_result__");
        }
    }
}

impl Default for ReactorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_state_initial() {
        let state = ReactorState::new();
        assert!(state.is_stable());
        assert_eq!(state.version, 0);
        assert_eq!(state.pending_io_count, 0);
        assert!(state.pending_requests.is_empty());
    }

    #[test]
    fn test_io_request_tracking() {
        let mut state = ReactorState::new();
        let id1 = FactId(1);
        let id2 = FactId(2);

        state.register_io_request(id1, IoType::CALL_EXTERNAL);
        assert_eq!(state.pending_io_count, 1);
        assert!(state.pending_requests.contains(&id1));

        state.register_io_request(id2, IoType::CALL_EXTERNAL);
        assert_eq!(state.pending_io_count, 2);
        assert!(state.pending_requests.contains(&id2));

        assert!(state.complete_io_request(id1));
        assert_eq!(state.pending_io_count, 1);
        assert!(!state.pending_requests.contains(&id1));

        // 完成不存在的请求
        assert!(!state.complete_io_request(FactId(999)));
        assert_eq!(state.pending_io_count, 1);
    }

    #[test]
    fn test_push_front_all_preserves_order() {
        let mut state = ReactorState::new();
        let instrs = vec![
            JsonValue::string("a"),
            JsonValue::string("b"),
            JsonValue::string("c"),
        ];
        state.push_front_all(instrs);

        assert_eq!(state.pop_instruction(), Some(JsonValue::string("a")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("b")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("c")));
    }

    #[test]
    fn test_push_back_all_preserves_order() {
        let mut state = ReactorState::new();
        let instrs = vec![
            JsonValue::string("x"),
            JsonValue::string("y"),
            JsonValue::string("z"),
        ];
        state.push_back_all(instrs);

        assert_eq!(state.pop_instruction(), Some(JsonValue::string("x")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("y")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("z")));
    }

    #[test]
    fn test_push_front_single() {
        let mut state = ReactorState::new();
        state.push_back(JsonValue::string("a"));
        state.push_back(JsonValue::string("b"));

        // push_front 将指令插入队首
        state.push_front(JsonValue::string("urgent"));

        assert_eq!(state.pop_instruction(), Some(JsonValue::string("urgent")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("a")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("b")));
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_push_front_all_to_empty_queue() {
        let mut state = ReactorState::new();
        state.push_front_all(vec![JsonValue::string("only")]);
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("only")));
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_push_front_all_interleaved_with_existing() {
        let mut state = ReactorState::new();
        state.push_back(JsonValue::string("old"));

        // push_front_all 应将新指令插入到 old 之前
        state.push_front_all(vec![JsonValue::string("new1"), JsonValue::string("new2")]);

        assert_eq!(state.pop_instruction(), Some(JsonValue::string("new1")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("new2")));
        assert_eq!(state.pop_instruction(), Some(JsonValue::string("old")));
    }

    #[test]
    fn test_register_duplicate_io_request() {
        // 重复注册同一 id 应幂等：count 与 set 大小保持一致
        let mut state = ReactorState::new();
        let id = FactId(1);

        state.register_io_request(id, IoType::CALL_EXTERNAL);
        assert_eq!(state.pending_io_count, 1);
        assert_eq!(state.pending_requests.len(), 1);

        // 重复注册同一 id：count 不变（幂等）
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        assert_eq!(state.pending_io_count, 1);
        assert_eq!(state.pending_requests.len(), 1);

        // 完成后：count 减为 0，set 为空
        assert!(state.complete_io_request(id));
        assert_eq!(state.pending_io_count, 0);
        assert!(state.pending_requests.is_empty());

        // 再次完成：返回 false
        assert!(!state.complete_io_request(id));
        assert_eq!(state.pending_io_count, 0);
    }

    #[test]
    fn test_queue_len() {
        let mut state = ReactorState::new();
        assert_eq!(state.queue_len(), 0);

        state.push_back(JsonValue::string("a"));
        assert_eq!(state.queue_len(), 1);

        state.push_back(JsonValue::string("b"));
        assert_eq!(state.queue_len(), 2);

        state.pop_instruction();
        assert_eq!(state.queue_len(), 1);
    }

    #[test]
    fn test_pop_from_empty_queue() {
        let mut state = ReactorState::new();
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_complete_io_request_empty() {
        let mut state = ReactorState::new();
        // 空状态下完成任意 id 应返回 false
        assert!(!state.complete_io_request(FactId(999)));
        assert_eq!(state.pending_io_count, 0);
    }

    #[test]
    fn test_default_state() {
        let state = ReactorState::default();
        assert!(state.is_stable());
        assert_eq!(state.version, 0);
        assert_eq!(state.queue_len(), 0);
    }

    // ===== P3-11 资源管理测试 =====

    #[test]
    fn test_register_io_request_records_timestamp() {
        // P3-11: register_io_request 应同时记录时间戳
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        assert!(state.pending_io_timestamps.contains_key(&id));
    }

    #[test]
    fn test_complete_io_request_removes_timestamp() {
        // P3-11: complete_io_request 应同时移除时间戳
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        assert!(state.pending_io_timestamps.contains_key(&id));
        assert!(state.complete_io_request(id));
        assert!(!state.pending_io_timestamps.contains_key(&id));
    }

    #[test]
    fn test_scan_io_timeouts_no_pending() {
        // P3-11: 无 pending I/O 时 scan 返回空
        let state = ReactorState::new();
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert!(warn_ids.is_empty());
        assert!(error_ids.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_warn_level() {
        // P3-11: 超过 warn_timeout 但未超过 error_timeout
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        // 模拟 35s 前注册的请求（超过 30s warn，未超过 60s error）
        state
            .pending_io_timestamps
            .insert(id, Instant::now() - Duration::from_secs(35));
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert_eq!(warn_ids, vec![FactId(1)]);
        assert!(error_ids.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_error_level() {
        // P3-11: 超过 error_timeout
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        // 模拟 65s 前注册的请求（超过 60s error）
        state
            .pending_io_timestamps
            .insert(id, Instant::now() - Duration::from_secs(65));
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        // 超过 error 阈值的不会同时出现在 warn 列表中
        assert!(warn_ids.is_empty());
        assert_eq!(error_ids, vec![FactId(1)]);
    }

    #[test]
    fn test_scan_io_timeouts_mixed() {
        // P3-11: 混合场景：一个 warn 级别，一个 error 级别，一个正常
        let mut state = ReactorState::new();
        let warn_id = FactId(1);
        let error_id = FactId(2);
        let normal_id = FactId(3);
        state.register_io_request(warn_id, IoType::CALL_EXTERNAL);
        state.register_io_request(error_id, IoType::CALL_EXTERNAL);
        state.register_io_request(normal_id, IoType::CALL_EXTERNAL);
        // 手动设置时间戳
        state
            .pending_io_timestamps
            .insert(warn_id, Instant::now() - Duration::from_secs(40));
        state
            .pending_io_timestamps
            .insert(error_id, Instant::now() - Duration::from_secs(70));
        // normal_id 保持当前时间
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert_eq!(warn_ids, vec![FactId(1)]);
        assert_eq!(error_ids, vec![FactId(2)]);
    }

    #[test]
    fn test_force_remove_io_request() {
        // P3-11 + 阶段3-1.4: force_remove_io_request 应清除所有相关状态
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        state.save_io_instruction(id, JsonValue::string("original_instruction"));

        // 确认状态已设置
        assert_eq!(state.pending_io_count, 1);
        assert!(state.pending_requests.contains(&id));
        assert!(state.pending_io_instructions.contains_key(&id));
        assert!(state.pending_io_timestamps.contains_key(&id));
        assert!(state.pending_io_types.contains_key(&id));

        // 强制移除
        state.force_remove_io_request(id);

        // 确认所有相关状态已清除
        assert_eq!(state.pending_io_count, 0);
        assert!(!state.pending_requests.contains(&id));
        assert!(!state.pending_io_instructions.contains_key(&id));
        assert!(!state.pending_io_timestamps.contains_key(&id));
        assert!(!state.pending_io_types.contains_key(&id));
    }

    #[test]
    fn test_force_remove_nonexistent_io_request() {
        // P3-11: 强制移除不存在的请求不应 panic
        let mut state = ReactorState::new();
        state.force_remove_io_request(FactId(999));
        assert_eq!(state.pending_io_count, 0);
    }

    // ===== 阶段3-1.4 I/O 类型记录测试 =====

    #[test]
    fn test_register_io_request_records_io_type() {
        // 阶段3-1.4: register_io_request 应同时记录 io_type
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::QUERY_DB);
        assert_eq!(state.pending_io_types.get(&id), Some(&IoType::QUERY_DB));
    }

    #[test]
    fn test_complete_io_request_removes_io_type() {
        // 阶段3-1.4: complete_io_request 应同时移除 io_type 记录
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        assert!(state.pending_io_types.contains_key(&id));
        assert!(state.complete_io_request(id));
        assert!(!state.pending_io_types.contains_key(&id));
    }

    #[test]
    fn test_register_multiple_io_types() {
        // 阶段3-1.4: 不同请求记录不同 io_type
        let mut state = ReactorState::new();
        state.register_io_request(FactId(1), IoType::CALL_EXTERNAL);
        state.register_io_request(FactId(2), IoType::QUERY_DB);
        state.register_io_request(FactId(3), IoType::HTTP_GET);

        assert_eq!(state.pending_io_types.len(), 3);
        assert_eq!(
            state.pending_io_types.get(&FactId(1)),
            Some(&IoType::CALL_EXTERNAL)
        );
        assert_eq!(
            state.pending_io_types.get(&FactId(2)),
            Some(&IoType::QUERY_DB)
        );
        assert_eq!(
            state.pending_io_types.get(&FactId(3)),
            Some(&IoType::HTTP_GET)
        );
    }
}
