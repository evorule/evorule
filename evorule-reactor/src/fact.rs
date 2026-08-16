// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 事实（Fact）定义 - 系统的原子通信单元

use evorule_tcb::JsonValue;

/// 事实唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub u64);

impl core::fmt::Display for FactId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "F{}", self.0)
    }
}

/// I/O 请求类型（与 core_eval.json 中的 io_type 对应）
///
/// v0.2.0：内部表示从 `&'static str` 改为 `Arc<str>`，支持应用层注册任意
/// io_type（如 `retrieve`/`file`/`http`）。`Arc<str>` 满足 `Clone + Eq + Hash +
/// Ord + Send + Sync`，可作 `BTreeMap`/`HashMap` key、跨线程共享、克隆廉价
/// （原子计数）。
///
/// # 失去 `Copy` 的影响
///
/// 所有按值传递处需显式 `.clone()`。典型场景：reactor 中
/// `state.register_io_request(id, io_type)` 后仍要在 `Fact::IoRequest` 与
/// `tracing::debug!` 中使用 `io_type`，需 `io_type.clone()`。
///
/// # 5 个旧 io_type
///
/// 从 `const` 改为工厂函数（`Arc::from` 非 const）。字符串值不变，故
/// `IoType::new("call_service") == IoType::call_service()` 成立，旧 WAL /
/// core_eval 无需改动。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IoType(pub std::sync::Arc<str>);

impl IoType {
    /// 调用外部服务
    pub fn call_external() -> Self {
        Self(std::sync::Arc::from("call_external"))
    }
    /// 查询数据库
    pub fn query_db() -> Self {
        Self(std::sync::Arc::from("query_db"))
    }
    /// HTTP GET 请求
    pub fn http_get() -> Self {
        Self(std::sync::Arc::from("http_get"))
    }
    /// 保存到记忆
    pub fn save_memory() -> Self {
        Self(std::sync::Arc::from("save_memory"))
    }
    /// 调用外部服务（v0.1.x 借道路径：按 `params.service_name` 二级路由）
    pub fn call_service() -> Self {
        Self(std::sync::Arc::from("call_service"))
    }

    /// 运行时构造任意 io_type（v0.2.0 自定义 IoType 入口）
    ///
    /// 用于应用层注册自定义 io_type，如 `IoType::new("retrieve")`。
    /// 5 个旧 io_type 字符串值不变：`IoType::new("call_service") == IoType::call_service()`。
    pub fn new(name: &str) -> Self {
        Self(std::sync::Arc::from(name))
    }

    /// 从字符串解析 I/O 类型
    ///
    /// v0.2.0：无条件接受（校验责任移到 subscriber / `ReactorBuilder::known_io_types`）。
    /// 保留方法名仅为向后兼容旧调用点，行为从"未知返回 None"变为"始终返回 Some"，
    /// 避免静默破坏。
    #[deprecated(note = "v0.2.0 起用 IoType::new；parse 不再校验，保留仅为向后兼容")]
    pub fn parse(s: &str) -> Option<Self> {
        Some(Self::new(s))
    }

    /// 转为字符串
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

/// 控制流指令类型（G8 合规：唯一真值来源，位于 fact.rs 受豁免）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlFlowType {
    /// 顺序执行多条指令
    Sequence,
    /// 条件分支
    Conditional,
    /// 循环执行
    WhileLoop,
    /// 将指令推入队列
    Push,
}

impl ControlFlowType {
    /// 从字符串解析控制流类型
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sequence" => Some(ControlFlowType::Sequence),
            "conditional" => Some(ControlFlowType::Conditional),
            "while_loop" => Some(ControlFlowType::WhileLoop),
            "push" => Some(ControlFlowType::Push),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            ControlFlowType::Sequence => "sequence",
            ControlFlowType::Conditional => "conditional",
            ControlFlowType::WhileLoop => "while_loop",
            ControlFlowType::Push => "push",
        }
    }
}

impl core::fmt::Display for IoType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 事实 ID 生成器（单调递增）
#[derive(Debug)]
pub struct FactIdGenerator {
    next: u64,
}

impl FactIdGenerator {
    /// 创建新的生成器（从 1 开始）
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// 从指定值恢复（崩溃恢复续跑用）
    ///
    /// `resume(n)` 后首次 `next_id()` 返回 `FactId(n)`。
    /// `resume(1)` 等价于 `new()`。
    ///
    /// 用于崩溃恢复场景：从历史 WAL 中的最大 FactId + 1 续跑，
    /// 避免新一轮产生的 FactId 与历史 WAL 中的同类型 Fact 重复。
    pub fn resume(from: u64) -> Self {
        Self { next: from }
    }

    /// 生成下一个 ID
    pub fn next_id(&mut self) -> FactId {
        let id = FactId(self.next);
        self.next += 1;
        id
    }
}

impl Default for FactIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// 事实（Fact）—— 系统的原子通信单元
///
/// 所有组件之间仅通过事实通信，无直接函数调用。
///
/// # 设计约定
/// - 所有变体携带 `id: FactId`，用于全局唯一标识与审计追踪
/// - `StateTransition` 和 `IoRequest` 携带 `cause: FactId`，指向触发该事实的来源
/// - `IoResponse` 携带 `error: Option<String>`，None 表示成功，Some 表示 I/O 失败
#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    /// 用户提交新指令（触发执行）
    Command {
        /// 事实唯一标识符
        id: FactId,
        /// 待执行的指令对象
        instruction: JsonValue,
    },

    /// 外部更新 payload 字段（由治理层注入）
    PayloadUpdate {
        /// 事实唯一标识符
        id: FactId,
        /// 要更新的 payload 路径
        path: String,
        /// 新值
        value: JsonValue,
    },

    /// 状态转换（由反应器自动产生）
    StateTransition {
        /// 事实唯一标识符
        id: FactId,
        /// 触发此转换的源事实 ID（通常是 Command 或 IoResponse）
        cause: FactId,
        /// 转换后的新 payload
        new_payload: JsonValue,
        /// 转换后的新指令队列
        new_queue: Vec<JsonValue>,
    },

    /// I/O 请求（由 TCB 产生，由治理层消费）
    IoRequest {
        /// 事实唯一标识符，用于 IoRequest ↔ IoResponse 配对
        id: FactId,
        /// 触发此 I/O 请求的源事实 ID
        cause: FactId,
        /// I/O 类型
        io_type: IoType,
        /// 请求参数
        params: JsonValue,
    },

    /// I/O 响应（由治理层产生，由反应器消费）
    IoResponse {
        /// 事实唯一标识符
        id: FactId,
        /// 对应的 IoRequest ID
        request_id: FactId,
        /// I/O 执行结果
        result: JsonValue,
        /// I/O 错误信息（None=成功，Some=失败描述）
        error: Option<String>,
    },

    /// 系统稳定（无更多指令可执行）
    Stable {
        /// 事实唯一标识符
        id: FactId,
        /// 最终的 payload 快照
        final_snapshot: JsonValue,
    },

    /// 系统错误（超时或 TCB 内部错误）
    Error {
        /// 事实唯一标识符
        id: FactId,
        /// 错误描述
        message: String,
    },
}

impl Fact {
    /// 返回事实类型的字符串名称（用于日志）
    pub fn type_name(&self) -> &'static str {
        match self {
            Fact::Command { .. } => "Command",
            Fact::PayloadUpdate { .. } => "PayloadUpdate",
            Fact::StateTransition { .. } => "StateTransition",
            Fact::IoRequest { .. } => "IoRequest",
            Fact::IoResponse { .. } => "IoResponse",
            Fact::Stable { .. } => "Stable",
            Fact::Error { .. } => "Error",
        }
    }

    /// 返回事实的唯一标识符
    pub fn id(&self) -> FactId {
        match self {
            Fact::Command { id, .. }
            | Fact::PayloadUpdate { id, .. }
            | Fact::StateTransition { id, .. }
            | Fact::IoRequest { id, .. }
            | Fact::IoResponse { id, .. }
            | Fact::Stable { id, .. }
            | Fact::Error { id, .. } => *id,
        }
    }

    /// 是否为终止事实（Stable 或 Error）
    pub fn is_terminal(&self) -> bool {
        matches!(self, Fact::Stable { .. } | Fact::Error { .. })
    }

    /// 将 Fact 序列化为 JsonValue（供 HTTP API 返回完整 Fact 详情）
    ///
    /// 返回的 JSON 对象包含 `type` 字段和各变体的所有字段，
    /// 可通过 `tcb_to_serde` 转为 `serde_json::Value` 用于 HTTP 响应。
    pub fn to_json(&self) -> JsonValue {
        use evorule_tcb::JsonValue as J;
        match self {
            Fact::Command { id, instruction } => J::object_from_pairs(&[
                ("type", J::string("Command")),
                ("id", J::integer(id.0 as i64)),
                ("instruction", instruction.clone()),
            ]),
            Fact::PayloadUpdate { id, path, value } => J::object_from_pairs(&[
                ("type", J::string("PayloadUpdate")),
                ("id", J::integer(id.0 as i64)),
                ("path", J::string(path.clone())),
                ("value", value.clone()),
            ]),
            Fact::StateTransition {
                id,
                cause,
                new_payload,
                new_queue,
            } => J::object_from_pairs(&[
                ("type", J::string("StateTransition")),
                ("id", J::integer(id.0 as i64)),
                ("cause", J::integer(cause.0 as i64)),
                ("new_payload", new_payload.clone()),
                ("new_queue", J::array(new_queue.clone())),
            ]),
            Fact::IoRequest {
                id,
                cause,
                io_type,
                params,
            } => J::object_from_pairs(&[
                ("type", J::string("IoRequest")),
                ("id", J::integer(id.0 as i64)),
                ("cause", J::integer(cause.0 as i64)),
                ("io_type", J::string(io_type.as_str())),
                ("params", params.clone()),
            ]),
            Fact::IoResponse {
                id,
                request_id,
                result,
                error,
            } => {
                let pairs: Vec<(&str, J)> = vec![
                    ("type", J::string("IoResponse")),
                    ("id", J::integer(id.0 as i64)),
                    ("request_id", J::integer(request_id.0 as i64)),
                    ("result", result.clone()),
                    (
                        "error",
                        match error {
                            Some(msg) => J::string(msg.clone()),
                            None => J::null(),
                        },
                    ),
                ];
                J::object_from_pairs(&pairs)
            }
            Fact::Stable { id, final_snapshot } => J::object_from_pairs(&[
                ("type", J::string("Stable")),
                ("id", J::integer(id.0 as i64)),
                ("final_snapshot", final_snapshot.clone()),
            ]),
            Fact::Error { id, message } => J::object_from_pairs(&[
                ("type", J::string("Error")),
                ("id", J::integer(id.0 as i64)),
                ("message", J::string(message.clone())),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_fact_id_generator() {
        let mut gen = FactIdGenerator::new();
        assert_eq!(gen.next_id(), FactId(1));
        assert_eq!(gen.next_id(), FactId(2));
        assert_eq!(gen.next_id(), FactId(3));
    }

    #[test]
    fn test_fact_id_generator_resume() {
        let mut gen = FactIdGenerator::resume(100);
        assert_eq!(gen.next_id(), FactId(100));
        assert_eq!(gen.next_id(), FactId(101));
        assert_eq!(gen.next_id(), FactId(102));
    }

    #[test]
    fn test_fact_id_generator_resume_from_1_equals_new() {
        let mut a = FactIdGenerator::new();
        let mut b = FactIdGenerator::resume(1);
        for _ in 0..5 {
            assert_eq!(a.next_id(), b.next_id());
        }
    }

    #[test]
    fn test_fact_type_name_and_id() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        assert_eq!(fact.type_name(), "Command");
        assert_eq!(fact.id(), FactId(1));

        let fact = Fact::Stable {
            id: FactId(2),
            final_snapshot: JsonValue::empty_object(),
        };
        assert_eq!(fact.type_name(), "Stable");
        assert_eq!(fact.id(), FactId(2));
        assert!(fact.is_terminal());
    }

    #[test]
    fn test_fact_cause_field() {
        let transition = Fact::StateTransition {
            id: FactId(10),
            cause: FactId(5),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };
        assert_eq!(transition.id(), FactId(10));

        let io_req = Fact::IoRequest {
            id: FactId(20),
            cause: FactId(10),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        };
        assert_eq!(io_req.id(), FactId(20));
    }

    #[test]
    fn test_io_response_with_error() {
        let ok_resp = Fact::IoResponse {
            id: FactId(1),
            request_id: FactId(2),
            result: JsonValue::string("ok"),
            error: None,
        };
        assert_eq!(ok_resp.type_name(), "IoResponse");
        assert!(!ok_resp.is_terminal());

        let err_resp = Fact::IoResponse {
            id: FactId(2),
            request_id: FactId(2),
            result: JsonValue::Null,
            error: Some("timeout".to_string()),
        };
        assert_eq!(err_resp.id(), FactId(2));
    }

    #[test]
    fn test_io_type_roundtrip() {
        // 所有 IoType 变体的 new ↔ as_str 往返
        for expected in [
            IoType::call_external(),
            IoType::query_db(),
            IoType::http_get(),
            IoType::save_memory(),
            IoType::call_service(),
        ] {
            let s = expected.as_str();
            let constructed = IoType::new(s);
            assert_eq!(constructed, expected, "roundtrip failed for {}", s);
        }
    }

    #[test]
    fn test_io_type_parse_accepts_any() {
        // v0.2.0: parse 无条件接受（校验责任移到 subscriber / known_io_types）
        #[allow(deprecated)]
        {
            assert!(IoType::parse("unknown").is_some());
            assert!(IoType::parse("retrieve").is_some());
            assert!(IoType::parse("call_service").is_some());
            // 空字符串也被接受（校验由上层负责）
            assert!(IoType::parse("").is_some());
        }
    }

    #[test]
    fn test_io_type_new_equals_factory() {
        // v0.2.0: new 构造与工厂函数对相同字符串应相等（HashMap/BTreeMap key 一致性）
        assert_eq!(IoType::new("call_service"), IoType::call_service());
        assert_eq!(IoType::new("call_external"), IoType::call_external());
        assert_eq!(IoType::new("retrieve"), IoType::new("retrieve"));
        assert_ne!(IoType::new("retrieve"), IoType::new("file"));
    }

    #[test]
    fn test_io_type_display() {
        assert_eq!(format!("{}", IoType::call_external()), "call_external");
        assert_eq!(format!("{}", IoType::query_db()), "query_db");
        assert_eq!(format!("{}", IoType::http_get()), "http_get");
        assert_eq!(format!("{}", IoType::save_memory()), "save_memory");
        assert_eq!(format!("{}", IoType::call_service()), "call_service");
    }

    #[test]
    fn test_fact_id_display() {
        assert_eq!(format!("{}", FactId(0)), "F0");
        assert_eq!(format!("{}", FactId(1)), "F1");
        assert_eq!(format!("{}", FactId(42)), "F42");
    }

    #[test]
    fn test_fact_is_terminal_all_variants() {
        // 终止事实
        assert!(Fact::Stable {
            id: FactId(1),
            final_snapshot: JsonValue::empty_object(),
        }
        .is_terminal());
        assert!(Fact::Error {
            id: FactId(2),
            message: "err".to_string(),
        }
        .is_terminal());

        // 非终止事实
        assert!(!Fact::Command {
            id: FactId(3),
            instruction: JsonValue::empty_object(),
        }
        .is_terminal());
        assert!(!Fact::PayloadUpdate {
            id: FactId(4),
            path: "x".to_string(),
            value: JsonValue::Null,
        }
        .is_terminal());
        assert!(!Fact::StateTransition {
            id: FactId(5),
            cause: FactId(0),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        }
        .is_terminal());
        assert!(!Fact::IoRequest {
            id: FactId(6),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        }
        .is_terminal());
        assert!(!Fact::IoResponse {
            id: FactId(7),
            request_id: FactId(6),
            result: JsonValue::Null,
            error: None,
        }
        .is_terminal());
    }

    #[test]
    fn test_fact_type_name_all_variants() {
        assert_eq!(
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            }
            .type_name(),
            "Command"
        );
        assert_eq!(
            Fact::PayloadUpdate {
                id: FactId(1),
                path: "x".to_string(),
                value: JsonValue::Null,
            }
            .type_name(),
            "PayloadUpdate"
        );
        assert_eq!(
            Fact::StateTransition {
                id: FactId(1),
                cause: FactId(0),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            }
            .type_name(),
            "StateTransition"
        );
        assert_eq!(
            Fact::IoRequest {
                id: FactId(1),
                cause: FactId(0),
                io_type: IoType::call_external(),
                params: JsonValue::empty_object(),
            }
            .type_name(),
            "IoRequest"
        );
        assert_eq!(
            Fact::IoResponse {
                id: FactId(1),
                request_id: FactId(0),
                result: JsonValue::Null,
                error: None,
            }
            .type_name(),
            "IoResponse"
        );
        assert_eq!(
            Fact::Stable {
                id: FactId(1),
                final_snapshot: JsonValue::empty_object(),
            }
            .type_name(),
            "Stable"
        );
        assert_eq!(
            Fact::Error {
                id: FactId(1),
                message: "e".to_string(),
            }
            .type_name(),
            "Error"
        );
    }

    #[test]
    fn test_fact_id_generator_default() {
        // Default 应从 0 开始，next_id 从 1 开始
        let mut gen = FactIdGenerator::default();
        assert_eq!(gen.next_id(), FactId(1));
    }
}
