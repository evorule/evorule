// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 事实（Fact）定义 - 系统的原子通信单元

use tier0_tcb::JsonValue;

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
/// 阶段3-1.4：实现 `Copy`，使 `register_io_request(id, io_type)` 后 `io_type`
/// 仍可在调用方使用（如 reactor 中需要在 register 后再用于 Fact::IoRequest）。
///
/// 阶段6：从硬编码枚举改为 String newtype，支持自定义 I/O 类型扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IoType(pub &'static str);

impl IoType {
    /// 调用外部服务
    pub const CALL_EXTERNAL: Self = IoType("call_external");
    /// 查询数据库
    pub const QUERY_DB: Self = IoType("query_db");
    /// HTTP GET 请求
    pub const HTTP_GET: Self = IoType("http_get");
    /// 保存到记忆
    pub const SAVE_MEMORY: Self = IoType("save_memory");
    /// 调用外部服务
    pub const CALL_SERVICE: Self = IoType("call_service");

    /// 从字符串解析 I/O 类型
    ///
    /// 与 core_eval.json 的 io_type 字段对应。
    /// 未知类型返回 None（v0.1.0：不再用 Box::leak 创建自定义类型，避免内存泄漏）。
    /// 自定义 IoType 支持将在 v0.2.0 通过 Arc<str> 实现。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "call_external" => Some(IoType("call_external")),
            "query_db" => Some(IoType("query_db")),
            "http_get" => Some(IoType("http_get")),
            "save_memory" => Some(IoType("save_memory")),
            "call_service" => Some(IoType("call_service")),
            _ => None,
        }
    }

    /// 转为字符串
    pub fn as_str(&self) -> &str {
        self.0
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_id_generator() {
        let mut gen = FactIdGenerator::new();
        assert_eq!(gen.next_id(), FactId(1));
        assert_eq!(gen.next_id(), FactId(2));
        assert_eq!(gen.next_id(), FactId(3));
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
            io_type: IoType::CALL_EXTERNAL,
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
        // 所有 IoType 变体的 parse ↔ as_str 往返
        for expected in [
            IoType::CALL_EXTERNAL,
            IoType::QUERY_DB,
            IoType::HTTP_GET,
            IoType::SAVE_MEMORY,
            IoType::CALL_SERVICE,
        ] {
            let s = expected.as_str();
            let parsed = IoType::parse(s).expect("roundtrip should succeed");
            assert_eq!(parsed, expected, "roundtrip failed for {}", s);
        }
    }

    #[test]
    fn test_io_type_parse_unknown() {
        // v0.1.0: 未知类型返回 None（不再 Box::leak 创建自定义类型）
        assert!(IoType::parse("unknown").is_none());
        assert!(IoType::parse("").is_none());
        assert!(IoType::parse("CALL_LLM").is_none());
    }

    #[test]
    fn test_io_type_display() {
        assert_eq!(format!("{}", IoType::CALL_EXTERNAL), "call_external");
        assert_eq!(format!("{}", IoType::QUERY_DB), "query_db");
        assert_eq!(format!("{}", IoType::HTTP_GET), "http_get");
        assert_eq!(format!("{}", IoType::SAVE_MEMORY), "save_memory");
        assert_eq!(format!("{}", IoType::CALL_SERVICE), "call_service");
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
            io_type: IoType::CALL_EXTERNAL,
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
                io_type: IoType::CALL_EXTERNAL,
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
