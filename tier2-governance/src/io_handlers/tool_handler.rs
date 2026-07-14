#![forbid(unsafe_code)]
//! Tool I/O Handler —— 工具注册与分发器。
//!
//! 通过 `register` 方法注册命名工具函数，运行时根据参数中的 `tool_name`
//! 查找并调用对应工具。工具函数签名为 `Fn(&JsonValue) -> Future<Output = IoResult>`，
//! 支持任意的异步副作用（如调用外部服务、计算等）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 工具函数类型
///
/// 一个线程安全的异步闭包：
/// - 输入：`&JsonValue`（工具参数）
/// - 输出：`IoResult`（工具执行结果）
///
/// 使用 `Pin<Box<dyn Future + Send>>` 以支持任意 `async` 块。
pub type ToolFn =
    Box<dyn Fn(&JsonValue) -> Pin<Box<dyn Future<Output = IoResult> + Send>> + Send + Sync>;

/// Tool 处理器
///
/// 持有已注册的工具函数表，根据 `tool_name` 分发执行。
pub struct ToolHandler {
    /// 工具函数映射表
    tools: HashMap<String, ToolFn>,
}

impl ToolHandler {
    /// 创建新的（空的）Tool 处理器。
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// 注册一个工具函数。
    ///
    /// # 参数
    /// - `name`: 工具名称（与 `IoRequest.params.tool_name` 对应）
    /// - `func`: 工具函数（装箱后的异步闭包）
    ///
    /// 若同名工具已存在，将被覆盖。
    pub fn register(&mut self, name: String, func: ToolFn) {
        self.tools.insert(name, func);
    }

    /// 查询已注册工具数量。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 检查是否注册了指定名称的工具。
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 判断是否为空。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl IoHandler for ToolHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 tool_name（必需）
        let tool_name = params
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: tool_name".to_string())?;

        // 提取 args（可选，默认 Null）
        let args = params.get("args").cloned().unwrap_or(JsonValue::Null);

        // 查找工具
        let func = self
            .tools
            .get(tool_name)
            .ok_or_else(|| format!("tool not found: {tool_name}"))?;

        // 调用工具函数并等待结果
        let future = func(&args);
        future.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_dispatch() {
        let mut handler = ToolHandler::new();
        // 注册一个 echo 工具
        handler.register(
            "echo".to_string(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        assert!(handler.contains("echo"));
        assert!(!handler.is_empty());
        assert_eq!(handler.len(), 1);

        let params = JsonValue::Object({
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                "tool_name".to_string(),
                JsonValue::String("echo".to_string()),
            );
            m.insert("args".to_string(), JsonValue::String("hello".to_string()));
            m
        });

        let result = handler.execute(&params).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JsonValue::String("hello".to_string()));
    }

    #[tokio::test]
    async fn test_tool_not_found() {
        let handler = ToolHandler::new();
        let params = JsonValue::Object({
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                "tool_name".to_string(),
                JsonValue::String("nonexistent".to_string()),
            );
            m
        });
        let result = handler.execute(&params).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("tool not found"));
    }

    #[tokio::test]
    async fn test_missing_tool_name() {
        let handler = ToolHandler::new();
        let params = JsonValue::Object(std::collections::BTreeMap::new());
        let result = handler.execute(&params).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("missing required param: tool_name"));
    }

    #[tokio::test]
    async fn test_default_args_is_null() {
        let mut handler = ToolHandler::new();
        // 注册一个返回 args 类型的工具
        handler.register(
            "check_null".to_string(),
            Box::new(|args: &JsonValue| {
                let is_null = args.is_null();
                Box::pin(async move { Ok(JsonValue::Bool(is_null)) })
            }),
        );

        let params = JsonValue::Object({
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                "tool_name".to_string(),
                JsonValue::String("check_null".to_string()),
            );
            m
        });

        let result = handler.execute(&params).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JsonValue::Bool(true));
    }
}
