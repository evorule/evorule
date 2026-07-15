#![forbid(unsafe_code)]
//! 工具注册表 —— 工具描述（OpenAI function calling 格式）+ 实现（ToolHandler）的统一注册。
//!
//! # 背景
//!
//! 现有 [`ToolHandler`](crate::io_handlers::tool_handler::ToolHandler) 只注册工具**实现**（异步闭包），
//! 缺少工具**描述**（JSON Schema），无法供 LLM function calling 使用。
//!
//! `ToolRegistry` 包装 `ToolHandler`，在同一注册接口中同时注册描述和实现：
//!
//! ```rust,ignore
//! let mut registry = ToolRegistry::new();
//! registry.register(
//!     "search_web".to_string(),
//!     "搜索网页获取信息".to_string(),
//!     json_schema,
//!     Box::new(|args| { ... }),
//! );
//! let openai_tools = registry.to_openai_tools(&["search_web".to_string()]);
//! let handler = registry.into_handler(); // 交给 IoDispatcher
//! ```
//!
//! # OpenAI function calling 格式
//!
//! `to_openai_tools()` 生成符合 OpenAI Chat Completions API 的 `tools` 参数：
//!
//! ```json
//! [
//!   {
//!     "type": "function",
//!     "function": {
//!       "name": "search_web",
//!       "description": "搜索网页获取信息",
//!       "parameters": { "type": "object", "properties": { ... } }
//!     }
//!   }
//! ]
//! ```

use std::collections::BTreeMap;
use std::collections::HashMap;

use tier0_tcb::JsonValue;

use crate::io_handlers::tool_handler::{ToolFn, ToolHandler};

/// 工具描述（OpenAI function calling 格式）
///
/// 描述工具的名称、功能和参数 schema，供 LLM 选择调用。
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// 工具名称（与 `call_tool` 指令的 `tool_name` 对应）
    pub name: String,
    /// 工具描述（供 LLM 理解工具用途）
    pub description: String,
    /// 参数 JSON Schema（描述工具接受的参数结构）
    ///
    /// 示例：`{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}`
    pub parameters: JsonValue,
}

impl ToolSpec {
    /// 创建新的工具描述
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: JsonValue,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    /// 转换为 OpenAI function calling 格式的单个工具对象
    ///
    /// 返回：`{"type":"function","function":{"name":...,"description":...,"parameters":...}}`
    pub fn to_openai_tool(&self) -> JsonValue {
        let mut function = BTreeMap::new();
        function.insert("name".to_string(), JsonValue::String(self.name.clone()));
        function.insert(
            "description".to_string(),
            JsonValue::String(self.description.clone()),
        );
        function.insert("parameters".to_string(), self.parameters.clone());

        let mut tool = BTreeMap::new();
        tool.insert(
            "type".to_string(),
            JsonValue::String("function".to_string()),
        );
        tool.insert("function".to_string(), JsonValue::Object(function));
        JsonValue::Object(tool)
    }
}

/// 工具注册表
///
/// 统一管理工具描述（`ToolSpec`）和工具实现（`ToolHandler`）。
/// Agent 层通过 `to_openai_tools()` 生成 LLM function calling 参数，
/// 通过 `into_handler()` 将实现交给 `IoDispatcher`。
pub struct ToolRegistry {
    /// 工具描述映射表（name → ToolSpec）
    specs: HashMap<String, ToolSpec>,
    /// 工具实现处理器
    handler: ToolHandler,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// 创建空的工具注册表
    pub fn new() -> Self {
        Self {
            specs: HashMap::new(),
            handler: ToolHandler::new(),
        }
    }

    /// 注册工具（同时注册描述和实现）
    ///
    /// # 参数
    /// - `name`: 工具名称
    /// - `description`: 工具描述（供 LLM 理解）
    /// - `parameters`: 参数 JSON Schema
    /// - `func`: 工具实现（异步闭包）
    ///
    /// 若同名工具已存在，描述和实现都会被覆盖。
    pub fn register(
        &mut self,
        name: String,
        description: String,
        parameters: JsonValue,
        func: ToolFn,
    ) {
        let spec = ToolSpec {
            name: name.clone(),
            description,
            parameters,
        };
        self.specs.insert(name.clone(), spec);
        self.handler.register(name, func);
    }

    /// 获取指定工具的描述
    pub fn get_spec(&self, name: &str) -> Option<&ToolSpec> {
        self.specs.get(name)
    }

    /// 获取多个工具的描述列表
    ///
    /// 按 `names` 顺序返回，跳过未注册的工具。
    pub fn get_tool_specs(&self, names: &[String]) -> Vec<&ToolSpec> {
        names.iter().filter_map(|n| self.specs.get(n)).collect()
    }

    /// 生成 OpenAI function calling 格式的 tools 参数
    ///
    /// # 参数
    /// - `names`: 需要包含的工具名称列表。传空切片时返回所有已注册工具。
    ///
    /// # 返回
    /// `JsonValue::Array`，每个元素为 `{"type":"function","function":{...}}`
    pub fn to_openai_tools(&self, names: &[String]) -> JsonValue {
        let tools: Vec<JsonValue> = if names.is_empty() {
            self.specs.values().map(|s| s.to_openai_tool()).collect()
        } else {
            names
                .iter()
                .filter_map(|n| self.specs.get(n))
                .map(|s| s.to_openai_tool())
                .collect()
        };
        JsonValue::Array(tools)
    }

    /// 获取所有已注册工具名称
    pub fn list_tool_names(&self) -> Vec<String> {
        self.specs.keys().cloned().collect()
    }

    /// 检查是否注册了指定工具
    pub fn contains(&self, name: &str) -> bool {
        self.specs.contains_key(name)
    }

    /// 已注册工具数量
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// 获取内部 ToolHandler 的引用（供 IoDispatcher 使用）
    pub fn handler(&self) -> &ToolHandler {
        &self.handler
    }

    /// 消耗 ToolRegistry，返回内部 ToolHandler（供 IoDispatcher 拥有所有权）
    pub fn into_handler(self) -> ToolHandler {
        self.handler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_handler::IoHandler;

    /// 创建简单的 echo 工具参数 schema
    fn echo_schema() -> JsonValue {
        let mut props = BTreeMap::new();
        let mut path_param = BTreeMap::new();
        path_param.insert("type".to_string(), JsonValue::String("string".to_string()));
        path_param.insert(
            "description".to_string(),
            JsonValue::String("要回声的文本".to_string()),
        );
        props.insert("text".to_string(), JsonValue::Object(path_param));

        let mut schema = BTreeMap::new();
        schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        schema.insert("properties".to_string(), JsonValue::Object(props));
        schema.insert(
            "required".to_string(),
            JsonValue::Array(vec![JsonValue::String("text".to_string())]),
        );
        JsonValue::Object(schema)
    }

    #[test]
    fn test_tool_spec_to_openai_tool() {
        let spec = ToolSpec::new("search_web", "搜索网页获取信息", echo_schema());

        let tool = spec.to_openai_tool();
        assert!(tool.is_object());

        // 验证 type = "function"
        assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("function"));

        // 验证 function 字段
        let function = tool.get("function").unwrap();
        assert_eq!(
            function.get("name").and_then(|v| v.as_str()),
            Some("search_web")
        );
        assert_eq!(
            function.get("description").and_then(|v| v.as_str()),
            Some("搜索网页获取信息")
        );
        assert!(function.get("parameters").is_some());
    }

    #[test]
    fn test_registry_new_is_empty() {
        let registry = ToolRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(!registry.contains("anything"));
    }

    #[test]
    fn test_registry_register_single() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声工具".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("echo"));

        let spec = registry.get_spec("echo").unwrap();
        assert_eq!(spec.name, "echo");
        assert_eq!(spec.description, "回声工具");
    }

    #[test]
    fn test_registry_register_multiple() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );
        registry.register(
            "add".to_string(),
            "加法".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("echo"));
        assert!(registry.contains("add"));

        let names = registry.list_tool_names();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_registry_register_overwrite() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "第一版".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );
        registry.register(
            "echo".to_string(),
            "第二版".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        assert_eq!(registry.len(), 1);
        let spec = registry.get_spec("echo").unwrap();
        assert_eq!(spec.description, "第二版");
    }

    #[test]
    fn test_to_openai_tools_all() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );
        registry.register(
            "search".to_string(),
            "搜索".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        // 空切片 → 返回所有工具
        let tools = registry.to_openai_tools(&[]);
        assert!(tools.is_array());
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // 每个工具都应有 type=function
        for tool in arr {
            assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("function"));
            assert!(tool.get("function").is_some());
        }
    }

    #[test]
    fn test_to_openai_tools_subset() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );
        registry.register(
            "search".to_string(),
            "搜索".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );
        registry.register(
            "write".to_string(),
            "写入".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );

        // 只选 echo 和 write
        let tools = registry.to_openai_tools(&["echo".to_string(), "write".to_string()]);
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        let names: Vec<&str> = arr
            .iter()
            .map(|t| {
                t.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap()
            })
            .collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"write"));
        assert!(!names.contains(&"search"));
    }

    #[test]
    fn test_to_openai_tools_skip_unregistered() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );

        // 请求包含未注册的工具
        let tools = registry.to_openai_tools(&["echo".to_string(), "nonexistent".to_string()]);
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 1); // 只有 echo，nonexistent 被跳过
    }

    #[test]
    fn test_to_openai_tools_empty_registry() {
        let registry = ToolRegistry::new();
        let tools = registry.to_openai_tools(&[]);
        assert!(tools.is_array());
        assert_eq!(tools.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_get_tool_specs_subset() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "a".to_string(),
            "工具 A".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );
        registry.register(
            "b".to_string(),
            "工具 B".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );
        registry.register(
            "c".to_string(),
            "工具 C".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );

        let specs = registry.get_tool_specs(&["a".to_string(), "c".to_string()]);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "a");
        assert_eq!(specs[1].name, "c");
    }

    #[test]
    fn test_get_tool_specs_with_unregistered() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "a".to_string(),
            "工具 A".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );

        let specs = registry.get_tool_specs(&["a".to_string(), "x".to_string()]);
        assert_eq!(specs.len(), 1); // 跳过未注册的 x
    }

    #[test]
    fn test_into_handler() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        let handler = registry.into_handler();
        assert!(handler.contains("echo"));
        assert_eq!(handler.len(), 1);
    }

    #[tokio::test]
    async fn test_registry_handler_executes_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo".to_string(),
            "回声".to_string(),
            echo_schema(),
            Box::new(|args: &JsonValue| {
                let cloned = args.clone();
                Box::pin(async move { Ok(cloned) })
            }),
        );

        // 通过 handler 引用执行工具
        let handler = registry.handler();
        let mut params = BTreeMap::new();
        params.insert(
            "tool_name".to_string(),
            JsonValue::String("echo".to_string()),
        );
        params.insert("args".to_string(), JsonValue::String("hello".to_string()));
        let result = handler.execute(&JsonValue::Object(params)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JsonValue::String("hello".to_string()));
    }

    #[test]
    fn test_openai_tool_format_complete() {
        // 完整验证 OpenAI tool 格式
        let mut registry = ToolRegistry::new();
        registry.register(
            "search_web".to_string(),
            "搜索网页获取信息".to_string(),
            echo_schema(),
            Box::new(|_| Box::pin(async { Ok(JsonValue::Null) })),
        );

        let tools = registry.to_openai_tools(&["search_web".to_string()]);
        let tool = &tools.as_array().unwrap()[0];

        // 顶层 type
        assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("function"));

        // function.name
        let function = tool.get("function").unwrap();
        assert_eq!(
            function.get("name").and_then(|v| v.as_str()),
            Some("search_web")
        );

        // function.description
        assert_eq!(
            function.get("description").and_then(|v| v.as_str()),
            Some("搜索网页获取信息")
        );

        // function.parameters（JSON Schema）
        let params = function.get("parameters").unwrap();
        assert_eq!(params.get("type").and_then(|v| v.as_str()), Some("object"));
        assert!(params.get("properties").is_some());
        assert!(params.get("required").is_some());
    }
}
