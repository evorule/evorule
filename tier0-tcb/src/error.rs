//! TCB 错误类型 - 纯计算错误，不含 I/O

use alloc::string::String;

/// TCB 错误类型
///
/// # 设计原则
/// - 不包含 `IoPause`（I/O 由反应器处理）
/// - 不包含 `MaxStepsExceeded`（由反应器控制）
/// - 所有错误均不 panic
///
/// # 代码示例
///
/// `TcbError` 由 `execute_transition` 与 `execute_meta_instruction` 返回，
/// 可通过 `match` 分支处理；所有变体均实现了 `Display`：
///
/// ```
/// use tier0_tcb::TcbError;
///
/// let e = TcbError::MissingField("operation");
/// assert_eq!(e.to_string(), "missing field: operation");
///
/// // 分支处理不同错误类型
/// match e {
///     TcbError::MissingField(f) => assert_eq!(f, "operation"),
///     _ => panic!("unexpected variant"),
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcbError {
    /// 指令中缺少必需字段
    MissingField(&'static str),

    /// 未知的元指令类型
    UnknownMetaInstruction(String),

    /// 未知的 set 操作类型
    UnknownOperation(String),

    /// 状态结构异常（如 `__exec__` 不存在）
    InvalidState,

    /// 类型不匹配（如 `add` 遇到非整数）
    InvalidType,

    /// 路径解析失败（包含失败路径）
    PathResolutionFailed(String),

    /// branch 嵌套深度超限（最大 64 层）
    NestingTooDeep,

    /// 指令列表为空（`push` 空列表、`branch` 空分支等）
    EmptyInstructionList,

    /// 整数运算溢出（`add`/`sub` 超出 i64 范围）
    IntegerOverflow,
}

impl core::fmt::Display for TcbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TcbError::MissingField(field) => write!(f, "missing field: {}", field),
            TcbError::UnknownMetaInstruction(ty) => write!(f, "unknown meta instruction: {}", ty),
            TcbError::UnknownOperation(op) => write!(f, "unknown operation: {}", op),
            TcbError::InvalidState => write!(f, "invalid state structure"),
            TcbError::InvalidType => write!(f, "invalid type"),
            TcbError::PathResolutionFailed(path) => write!(f, "path resolution failed: {}", path),
            TcbError::NestingTooDeep => write!(f, "branch nesting depth exceeds limit (64)"),
            TcbError::EmptyInstructionList => write!(f, "empty instruction list"),
            TcbError::IntegerOverflow => write!(f, "integer arithmetic overflow"),
        }
    }
}

#[cfg(feature = "std")]
mod std_impls {
    extern crate std;
    use super::TcbError;
    impl std::error::Error for TcbError {}
}


#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    /// 验证 Display 实现对 9 个 variant 各自输出正确字符串
    #[test]
    fn test_display_all_variants() {
        assert_eq!(
            format!("{}", TcbError::MissingField("value")),
            "missing field: value"
        );
        assert_eq!(
            format!("{}", TcbError::UnknownMetaInstruction("foo".to_string())),
            "unknown meta instruction: foo"
        );
        assert_eq!(
            format!("{}", TcbError::UnknownOperation("bar".to_string())),
            "unknown operation: bar"
        );
        assert_eq!(
            format!("{}", TcbError::InvalidState),
            "invalid state structure"
        );
        assert_eq!(
            format!("{}", TcbError::InvalidType),
            "invalid type"
        );
        assert_eq!(
            format!("{}", TcbError::PathResolutionFailed("x.y".to_string())),
            "path resolution failed: x.y"
        );
        assert_eq!(
            format!("{}", TcbError::NestingTooDeep),
            "branch nesting depth exceeds limit (64)"
        );
        assert_eq!(
            format!("{}", TcbError::EmptyInstructionList),
            "empty instruction list"
        );
        assert_eq!(
            format!("{}", TcbError::IntegerOverflow),
            "integer arithmetic overflow"
        );
    }

    /// Debug trait 派生自 PartialEq, 验证 PartialEq 行为
    #[test]
    fn test_partial_eq_same_variant() {
        assert_eq!(TcbError::InvalidState, TcbError::InvalidState);
        assert_ne!(TcbError::InvalidState, TcbError::InvalidType);
        assert_eq!(
            TcbError::MissingField("x"),
            TcbError::MissingField("x")
        );
        assert_ne!(
            TcbError::MissingField("x"),
            TcbError::MissingField("y")
        );
    }
}
