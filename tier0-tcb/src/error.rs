//! TCB 错误类型 - 纯计算错误，不含 I/O

use alloc::string::String;

/// TCB 错误类型
///
/// # 设计原则
/// - 不包含 `IoPause`（I/O 由反应器处理）
/// - 不包含 `MaxStepsExceeded`（由反应器控制）
/// - 所有错误均不 panic
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
