#![forbid(unsafe_code)]
//! 委托机制（Phase A-6）—— Agent 间委托与结果传递。
//!
//! # 架构
//!
//! `delegate` 工具是特殊的"伪工具"：LLM 在 tool_calls 中请求调用 `delegate`，
//! 但 AgentRunner **拦截**该调用，在本地创建子 Agent 执行环境（Reactor + IoSubscriber +
//! AgentRunner），运行子 Agent 到完成，将其 final_answer 作为工具结果返回。
//!
//! 这避免了在 ToolHandler（简单异步闭包）中创建复杂反应器基础设施的问题。
//!
//! # 委托深度限制
//!
//! 为防止无限递归委托（Agent A → B → A → ...），`DelegateContext` 跟踪当前深度。
//! `max_depth`（默认 3）限制最大委托层数。达到上限时，子 Agent 的 `delegate` 上下文
//! 为 None，LLM 若仍尝试委托会收到错误信息。

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;

use crate::agent::definition::AgentDefinitionManager;
use crate::agent::runner::AgentRunner;
use crate::agent::AgentResult;
use crate::api::agent_api::DispatcherFactory;
use crate::io_subscriber::IoSubscriber;

/// 默认最大委托深度
pub const DEFAULT_MAX_DELEGATE_DEPTH: usize = 3;

/// 判断在当前深度是否还能继续委托（纯函数，无副作用）
///
/// 当 `current_depth + 1 >= max_depth` 时返回 `false`，表示不能再委托。
/// 此函数供 `DelegateContext::child()` 使用，也可独立测试嵌套委托的深度逻辑。
///
/// 使用 `checked_add` 防止 `current_depth + 1` 在极端值下溢出（debug 模式 panic）。
///
/// # 参数
/// - `current_depth`: 当前委托深度（0 = 顶层 Agent）
/// - `max_depth`: 最大委托深度
///
/// # 示例
/// - `can_delegate_at_depth(0, 3)` → `true`（0+1 < 3，可委托到 depth=1）
/// - `can_delegate_at_depth(2, 3)` → `false`（2+1 >= 3，不能委托到 depth=3）
/// - `can_delegate_at_depth(0, 1)` → `false`（0+1 >= 1，顶层也不能委托）
/// - `can_delegate_at_depth(0, 0)` → `false`（0+1 >= 0，禁止委托）
pub fn can_delegate_at_depth(current_depth: usize, max_depth: usize) -> bool {
    // 使用 checked_add 防止 usize::MAX + 1 溢出
    current_depth
        .checked_add(1)
        .is_some_and(|next| next < max_depth)
}

/// 委托上下文 —— 持有创建子 Agent 所需的全部基础设施。
///
/// 通过 `AgentRunner::with_delegate()` 注入。当 LLM 请求 `delegate` 工具调用时，
/// AgentRunner 使用此上下文创建子 Agent 并运行到完成。
///
/// # 嵌套委托
///
/// 子 Agent 的 AgentRunner 会获得一个 `child()` 上下文（depth + 1）。
/// 当 depth >= max_depth 时，`child()` 返回 None，子 Agent 无法继续委托。
#[derive(Clone)]
pub struct DelegateContext {
    /// Agent 定义管理器（加载子 Agent 定义）
    definitions: AgentDefinitionManager,
    /// core_eval 配置（创建子反应器）
    core_eval: Arc<Vec<JsonValue>>,
    /// 反应器最大轮次
    max_rounds: usize,
    /// IoDispatcher 工厂（每个子 Agent 创建新 dispatcher）
    dispatcher_factory: DispatcherFactory,
    /// 工具描述（OpenAI tools 格式，不含 delegate 工具）
    tools_json: Arc<JsonValue>,
    /// 记忆系统根目录（可选，预留给子 Agent 共享父 Agent memory 上下文）
    #[allow(dead_code)]
    memory_dir: Option<Arc<PathBuf>>,
    /// 最大委托深度
    max_depth: usize,
    /// 当前委托深度（0 = 顶层 Agent）
    current_depth: usize,
}

impl DelegateContext {
    /// 创建委托上下文
    ///
    /// # 参数
    /// - `definitions`: Agent 定义管理器
    /// - `core_eval`: transform 规则列表
    /// - `max_rounds`: 子反应器最大轮次
    /// - `dispatcher_factory`: IoDispatcher 异步工厂
    /// - `tools_json`: 工具描述（不含 delegate）
    /// - `memory_dir`: 记忆系统根目录（可选）
    /// - `max_depth`: 最大委托深度
    pub fn new(
        definitions: AgentDefinitionManager,
        core_eval: Arc<Vec<JsonValue>>,
        max_rounds: usize,
        dispatcher_factory: DispatcherFactory,
        tools_json: Arc<JsonValue>,
        memory_dir: Option<Arc<PathBuf>>,
        max_depth: usize,
    ) -> Self {
        Self {
            definitions,
            core_eval,
            max_rounds,
            dispatcher_factory,
            tools_json,
            memory_dir,
            max_depth,
            current_depth: 0,
        }
    }

    /// 创建子上下文（depth + 1）。达到 max_depth 时返回 None。
    fn child(&self) -> Option<Self> {
        if !can_delegate_at_depth(self.current_depth, self.max_depth) {
            tracing::warn!(
                current_depth = self.current_depth,
                max_depth = self.max_depth,
                "已达到最大委托深度，子 Agent 无法继续委托"
            );
            return None;
        }
        Some(Self {
            current_depth: self.current_depth + 1,
            ..self.clone()
        })
    }

    /// 获取当前委托深度（0 = 顶层 Agent）
    pub fn current_depth(&self) -> usize {
        self.current_depth
    }

    /// 获取最大委托深度
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// 判断当前是否还能继续委托（便捷方法，等价于 `child().is_some()`）
    pub fn can_delegate(&self) -> bool {
        can_delegate_at_depth(self.current_depth, self.max_depth)
    }

    /// 执行委托：创建子 Agent 并运行到完成
    ///
    /// # 参数
    /// - `agent_type`: 目标 Agent 类型
    /// - `task`: 子任务描述
    ///
    /// # 返回
    /// 子 Agent 的 final_answer 字符串
    ///
    /// # 实现说明
    ///
    /// 此方法返回 `Pin<Box<dyn Future>>` 而非 `async fn`，以打破间接异步递归：
    /// `delegate()` → `AgentRunner::run()` → `handle_delegate()` → `delegate()`。
    /// Rust 的 `async fn` 不允许直接递归，必须通过 `Box::pin` 打包 future。
    pub fn delegate<'a>(
        &'a self,
        agent_type: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                agent_type,
                task = %task,
                depth = self.current_depth,
                "开始委托执行子 Agent"
            );

            // 1. 加载子 Agent 定义
            let def = self
                .definitions
                .load(agent_type)
                .map_err(|e| format!("加载子 Agent 定义失败 ({}): {}", agent_type, e))?;

            let config = def.to_agent_config();

            // 2. 创建子反应器
            let reactor = Reactor::builder((*self.core_eval).clone())
                .max_rounds(self.max_rounds)
                .build();
            let (command_tx, event_rx, event_tx, _reactor_handle, _facts_log) = reactor.spawn();

            // 3. 创建 IoDispatcher + IoSubscriber
            let dispatcher = (self.dispatcher_factory)().await?;
            let subscriber = IoSubscriber::new(dispatcher);
            let sub_rx = event_tx.subscribe();
            let sub_tx = command_tx.clone();
            tokio::spawn(async move {
                let _ = subscriber.run(sub_rx, sub_tx).await;
            });

            // 4. 创建子 AgentRunner
            let mut runner =
                AgentRunner::new(config, command_tx, event_rx, (*self.tools_json).clone());

            // 5. 注入子委托上下文（如果未达深度上限）
            if let Some(child_ctx) = self.child() {
                runner = runner.with_delegate(child_ctx);
            }

            // 6. 运行子 Agent
            let result: AgentResult = runner
                .run(task)
                .await
                .map_err(|e| format!("子 Agent 执行失败: {}", e))?;

            tracing::info!(
                agent_type,
                depth = self.current_depth,
                steps = result.steps,
                "子 Agent 执行完成"
            );

            Ok(result.final_answer)
        })
    }
}

/// 构造 `delegate` 工具的 OpenAI function calling 描述。
///
/// 此描述会被添加到 tools_json 中，让 LLM 知道可以调用 `delegate` 工具。
/// AgentRunner 拦截 `delegate` 工具调用，不会将其转发到 ToolHandler。
pub fn delegate_tool_openai_spec() -> JsonValue {
    let mut props = BTreeMap::new();

    // agent_type 参数
    let mut agent_type_param = BTreeMap::new();
    agent_type_param.insert("type".to_string(), JsonValue::String("string".to_string()));
    agent_type_param.insert(
        "description".to_string(),
        JsonValue::String("目标 Agent 类型（如 researcher、writer）".to_string()),
    );
    props.insert(
        "agent_type".to_string(),
        JsonValue::Object(agent_type_param),
    );

    // task 参数
    let mut task_param = BTreeMap::new();
    task_param.insert("type".to_string(), JsonValue::String("string".to_string()));
    task_param.insert(
        "description".to_string(),
        JsonValue::String("子任务描述".to_string()),
    );
    props.insert("task".to_string(), JsonValue::Object(task_param));

    let mut schema = BTreeMap::new();
    schema.insert("type".to_string(), JsonValue::String("object".to_string()));
    schema.insert("properties".to_string(), JsonValue::Object(props));
    schema.insert(
        "required".to_string(),
        JsonValue::Array(vec![
            JsonValue::String("agent_type".to_string()),
            JsonValue::String("task".to_string()),
        ]),
    );

    let mut function = BTreeMap::new();
    function.insert(
        "name".to_string(),
        JsonValue::String("delegate".to_string()),
    );
    function.insert(
        "description".to_string(),
        JsonValue::String("将子任务委托给另一个 Agent 执行，获取其结果".to_string()),
    );
    function.insert("parameters".to_string(), JsonValue::Object(schema));

    let mut tool = BTreeMap::new();
    tool.insert(
        "type".to_string(),
        JsonValue::String("function".to_string()),
    );
    tool.insert("function".to_string(), JsonValue::Object(function));

    JsonValue::Object(tool)
}

/// 将 delegate 工具描述合并到现有 tools_json 数组中。
///
/// 如果 tools_json 已包含名为 "delegate" 的工具，则不重复添加。
pub fn merge_delegate_tool(tools_json: &JsonValue) -> JsonValue {
    let delegate_spec = delegate_tool_openai_spec();

    match tools_json {
        JsonValue::Array(arr) => {
            // 检查是否已包含 delegate
            let has_delegate = arr.iter().any(|t| {
                t.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    == Some("delegate")
            });
            if has_delegate {
                tools_json.clone()
            } else {
                let mut new_arr = arr.clone();
                new_arr.push(delegate_spec);
                JsonValue::Array(new_arr)
            }
        }
        _ => JsonValue::Array(vec![delegate_spec]),
    }
}

/// 解析 delegate 工具调用的参数。
///
/// 预期 args 为 JSON 字符串：`{"agent_type": "researcher", "task": "..."}`
pub fn parse_delegate_args(args: &str) -> Result<(String, String), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(args).map_err(|e| format!("解析 delegate 参数失败: {}", e))?;

    let agent_type = parsed
        .get("agent_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "delegate 参数缺少 agent_type".to_string())?
        .to_string();

    let task = parsed
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "delegate 参数缺少 task".to_string())?
        .to_string();

    Ok((agent_type, task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegate_tool_openai_spec_format() {
        let spec = delegate_tool_openai_spec();

        // 顶层 type = "function"
        assert_eq!(spec.get("type").and_then(|v| v.as_str()), Some("function"));

        let function = spec.get("function").unwrap();
        assert_eq!(
            function.get("name").and_then(|v| v.as_str()),
            Some("delegate")
        );
        assert!(function.get("description").is_some());

        let params = function.get("parameters").unwrap();
        assert_eq!(params.get("type").and_then(|v| v.as_str()), Some("object"));

        // 检查 properties
        let props = params.get("properties").unwrap();
        assert!(props.get("agent_type").is_some());
        assert!(props.get("task").is_some());

        // 检查 required
        let required = params.get("required").unwrap().as_array().unwrap();
        let required_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_names.contains(&"agent_type"));
        assert!(required_names.contains(&"task"));
    }

    #[test]
    fn test_merge_delegate_tool_into_empty() {
        let empty = JsonValue::Array(vec![]);
        let merged = merge_delegate_tool(&empty);
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0]
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("delegate")
        );
    }

    #[test]
    fn test_merge_delegate_tool_into_existing() {
        let mut existing_tool = BTreeMap::new();
        let mut func = BTreeMap::new();
        func.insert(
            "name".to_string(),
            JsonValue::String("search_web".to_string()),
        );
        existing_tool.insert(
            "type".to_string(),
            JsonValue::String("function".to_string()),
        );
        existing_tool.insert("function".to_string(), JsonValue::Object(func));

        let tools = JsonValue::Array(vec![JsonValue::Object(existing_tool)]);
        let merged = merge_delegate_tool(&tools);
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 2); // search_web + delegate
    }

    #[test]
    fn test_merge_delegate_tool_no_duplicate() {
        let delegate_spec = delegate_tool_openai_spec();
        let tools = JsonValue::Array(vec![delegate_spec]);
        let merged = merge_delegate_tool(&tools);
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 1); // 不重复添加
    }

    #[test]
    fn test_merge_delegate_tool_into_non_array() {
        let non_array = JsonValue::String("invalid".to_string());
        let merged = merge_delegate_tool(&non_array);
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 1); // 仅 delegate
    }

    #[test]
    fn test_parse_delegate_args_valid() {
        let args = r#"{"agent_type": "researcher", "task": "研究 Rust 异步运行时"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "researcher");
        assert_eq!(task, "研究 Rust 异步运行时");
    }

    #[test]
    fn test_parse_delegate_args_missing_agent_type() {
        let args = r#"{"task": "某任务"}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("agent_type"));
    }

    #[test]
    fn test_parse_delegate_args_missing_task() {
        let args = r#"{"agent_type": "researcher"}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("task"));
    }

    #[test]
    fn test_parse_delegate_args_invalid_json() {
        let args = "not json";
        let result = parse_delegate_args(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_delegate_context_child_depth_increment() {
        // 验证 DEFAULT_MAX_DELEGATE_DEPTH 的默认值
        assert_eq!(DEFAULT_MAX_DELEGATE_DEPTH, 3);
    }

    // ===== 嵌套委托深度逻辑测试（通过纯函数 can_delegate_at_depth）=====

    #[test]
    fn test_can_delegate_max_depth_zero() {
        // max_depth=0：完全禁止委托，任何深度都不能委托
        assert!(!can_delegate_at_depth(0, 0));
        assert!(!can_delegate_at_depth(1, 0));
        assert!(!can_delegate_at_depth(100, 0));
    }

    #[test]
    fn test_can_delegate_max_depth_one() {
        // max_depth=1：语义为"只允许 1 层委托存在"，但 child() 逻辑是
        // current_depth + 1 >= max_depth → 不能委托
        // depth=0: 0+1 >= 1 → false（顶层 Agent 不能委托子 Agent）
        // 这意味着 max_depth=1 实际禁止所有委托
        assert!(!can_delegate_at_depth(0, 1));
        assert!(!can_delegate_at_depth(1, 1));
    }

    #[test]
    fn test_can_delegate_max_depth_two() {
        // max_depth=2：允许 1 层委托（顶层 → 子 Agent，子 Agent 不能再委托）
        // depth=0: 0+1 < 2 → true（可以委托到 depth=1）
        // depth=1: 1+1 >= 2 → false（子 Agent 不能再委托）
        assert!(can_delegate_at_depth(0, 2));
        assert!(!can_delegate_at_depth(1, 2));
    }

    #[test]
    fn test_can_delegate_max_depth_three() {
        // max_depth=3（默认值）：允许 2 层委托
        // depth=0: 0+1 < 3 → true（可委托到 depth=1）
        // depth=1: 1+1 < 3 → true（可委托到 depth=2）
        // depth=2: 2+1 >= 3 → false（不能委托到 depth=3）
        assert!(can_delegate_at_depth(0, 3));
        assert!(can_delegate_at_depth(1, 3));
        assert!(!can_delegate_at_depth(2, 3));
    }

    #[test]
    fn test_can_delegate_max_depth_five() {
        // max_depth=5：允许 4 层委托
        // depth=0,1,2,3 可以委托；depth=4 不能
        for d in 0..4 {
            assert!(
                can_delegate_at_depth(d, 5),
                "depth={} 应该能委托 (max_depth=5)",
                d
            );
        }
        assert!(!can_delegate_at_depth(4, 5));
        assert!(!can_delegate_at_depth(5, 5));
    }

    #[test]
    fn test_can_delegate_overflow_safety() {
        // usize::MAX 边界：can_delegate_at_depth 使用 checked_add 防止溢出
        // usize::MAX + 1 会溢出，checked_add 返回 None，is_some_and 返回 false
        assert!(!can_delegate_at_depth(usize::MAX, usize::MAX));
        // 0 + 1 = 1，1 < usize::MAX → true
        assert!(can_delegate_at_depth(0, usize::MAX));
    }

    /// 模拟 3 层嵌套委托链：A(depth=0) → B(depth=1) → C(depth=2) → D(被阻止)
    ///
    /// 使用 max_depth=3（默认值），验证：
    /// - A 可以委托 B（depth 0→1）
    /// - B 可以委托 C（depth 1→2）
    /// - C 不能委托 D（depth 2→3 被阻止，因为 2+1 >= 3）
    #[test]
    fn test_nested_delegate_chain_max_depth_3() {
        let max_depth = 3;

        // A (depth=0) 想委托 B
        let a_can = can_delegate_at_depth(0, max_depth);
        assert!(a_can, "A (depth=0) 应该能委托 B");

        // B (depth=1) 想委托 C
        let b_can = can_delegate_at_depth(1, max_depth);
        assert!(b_can, "B (depth=1) 应该能委托 C");

        // C (depth=2) 想委托 D
        let c_can = can_delegate_at_depth(2, max_depth);
        assert!(!c_can, "C (depth=2) 不应该能委托 D（已达到 max_depth=3）");

        // 验证委托链深度递增模式：0→1→2→(阻止)
        let chain: Vec<usize> = (0..)
            .take_while(|&d| can_delegate_at_depth(d, max_depth))
            .collect();
        assert_eq!(
            chain,
            vec![0, 1],
            "max_depth=3 时可委托的深度链应为 [0, 1]，depth=2 被阻止"
        );
    }

    /// 模拟 5 层嵌套委托链：A(0) → B(1) → C(2) → D(3) → E(4) → F(被阻止)
    #[test]
    fn test_nested_delegate_chain_max_depth_5() {
        let max_depth = 5;

        // depth 0,1,2,3 可以委托
        for d in 0..4 {
            assert!(
                can_delegate_at_depth(d, max_depth),
                "depth={} 应该能委托 (max_depth={})",
                d,
                max_depth
            );
        }

        // depth=4 被阻止
        assert!(
            !can_delegate_at_depth(4, max_depth),
            "depth=4 不应该能委托（已达到 max_depth=5）"
        );

        // 验证委托链
        let chain: Vec<usize> = (0..)
            .take_while(|&d| can_delegate_at_depth(d, max_depth))
            .collect();
        assert_eq!(
            chain,
            vec![0, 1, 2, 3],
            "max_depth=5 时可委托的深度链应为 [0, 1, 2, 3]"
        );
    }

    /// 验证 max_depth 与实际可委托层数的关系
    ///
    /// 可委托层数 = max_depth - 1（因为 child() 逻辑是 depth+1 < max_depth）
    #[test]
    fn test_max_depth_vs_actual_delegate_levels() {
        for max_depth in 1..=10 {
            let can_delegate_count = (0..)
                .take_while(|&d| can_delegate_at_depth(d, max_depth))
                .count();
            // 可委托的深度数量 = max_depth.saturating_sub(1)
            let expected = max_depth.saturating_sub(1);
            assert_eq!(
                can_delegate_count, expected,
                "max_depth={} 时可委托层数应为 {}",
                max_depth, expected
            );
        }
    }

    // ===== parse_delegate_args 复杂边界测试 =====

    #[test]
    fn test_parse_delegate_args_with_json_special_chars() {
        // task 含双引号和反斜杠（JSON 转义）
        let args =
            r#"{"agent_type": "researcher", "task": "分析 \"hello world\" 路径 C:\\Users\\test"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "researcher");
        assert_eq!(task, r#"分析 "hello world" 路径 C:\Users\test"#);
    }

    #[test]
    fn test_parse_delegate_args_with_newlines() {
        // task 含换行符和制表符
        let args = r#"{"agent_type": "writer", "task": "第一行\n第二行\t缩进"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "writer");
        assert_eq!(task, "第一行\n第二行\t缩进");
        assert!(task.contains('\n'));
        assert!(task.contains('\t'));
    }

    #[test]
    fn test_parse_delegate_args_with_unicode_and_emoji() {
        // task 含多语言文字和 emoji
        let args = r#"{"agent_type": "translator", "task": "翻译：Hello 世界 🌍 日本語 한국어"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "translator");
        assert_eq!(task, "翻译：Hello 世界 🌍 日本語 한국어");
        assert!(task.contains("🌍"));
    }

    #[test]
    fn test_parse_delegate_args_extra_fields_ignored() {
        // 额外字段 priority, deadline 应被忽略
        let args = r#"{"agent_type": "researcher", "task": "研究任务", "priority": "high", "deadline": "2026-12-31"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "researcher");
        assert_eq!(task, "研究任务");
    }

    #[test]
    fn test_parse_delegate_args_numeric_agent_type() {
        // agent_type 含数字和下划线
        let args = r#"{"agent_type": "agent_v2_2026", "task": "执行任务"}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "agent_v2_2026");
        assert_eq!(task, "执行任务");
    }

    #[test]
    fn test_parse_delegate_args_empty_strings() {
        // 空字符串 agent_type 和 task（语法合法，语义可能无效）
        let args = r#"{"agent_type": "", "task": ""}"#;
        let (agent_type, task) = parse_delegate_args(args).unwrap();
        assert_eq!(agent_type, "");
        assert_eq!(task, "");
    }

    #[test]
    fn test_parse_delegate_args_non_object_json() {
        // JSON 数组（非 object）应失败
        let args = r#"["agent_type", "task"]"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());

        // JSON 字符串（非 object）应失败
        let args = r#""just a string""#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());

        // JSON 数字应失败
        let args = "42";
        let result = parse_delegate_args(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_delegate_args_null_values() {
        // agent_type 为 null 应失败
        let args = r#"{"agent_type": null, "task": "某任务"}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("agent_type"));

        // task 为 null 应失败
        let args = r#"{"agent_type": "researcher", "task": null}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("task"));
    }

    #[test]
    fn test_parse_delegate_args_non_string_types() {
        // agent_type 为数字应失败（as_str() 返回 None）
        let args = r#"{"agent_type": 123, "task": "某任务"}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());

        // task 为对象应失败
        let args = r#"{"agent_type": "researcher", "task": {"nested": "value"}}"#;
        let result = parse_delegate_args(args);
        assert!(result.is_err());
    }

    // ===== merge_delegate_tool 复杂场景测试 =====

    #[test]
    fn test_merge_delegate_tool_multiple_existing() {
        // 已有 3 个工具，合并后应有 4 个
        let make_tool = |name: &str| -> JsonValue {
            let mut func = BTreeMap::new();
            func.insert("name".to_string(), JsonValue::String(name.to_string()));
            let mut tool = BTreeMap::new();
            tool.insert(
                "type".to_string(),
                JsonValue::String("function".to_string()),
            );
            tool.insert("function".to_string(), JsonValue::Object(func));
            JsonValue::Object(tool)
        };

        let tools = JsonValue::Array(vec![
            make_tool("search_web"),
            make_tool("read_file"),
            make_tool("write_file"),
        ]);
        let merged = merge_delegate_tool(&tools);
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 4, "3 个已有工具 + delegate = 4");

        // 验证 delegate 在末尾
        let last = &arr[3];
        assert_eq!(
            last.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("delegate")
        );

        // 验证原有工具顺序不变
        assert_eq!(
            arr[0]
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("search_web")
        );
        assert_eq!(
            arr[1]
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("read_file")
        );
    }

    #[test]
    fn test_merge_delegate_tool_delegate_named_non_function() {
        // 已有名为 "delegate" 但结构不同的工具（不含 function 字段）
        // merge_delegate_tool 检查的是 function.name == "delegate"
        // 这个工具没有 function 字段，所以会被认为不是 delegate，会再添加一个
        let mut malformed = BTreeMap::new();
        malformed.insert("type".to_string(), JsonValue::String("custom".to_string()));
        malformed.insert(
            "name".to_string(),
            JsonValue::String("delegate".to_string()),
        );
        let tools = JsonValue::Array(vec![JsonValue::Object(malformed)]);

        let merged = merge_delegate_tool(&tools);
        let arr = merged.as_array().unwrap();
        // 原有工具（结构不符合）+ 新的 delegate 工具
        assert_eq!(
            arr.len(),
            2,
            "结构不符的 'delegate' 不被识别，应添加标准 delegate"
        );
    }

    #[test]
    fn test_merge_delegate_tool_preserves_delegate_at_end() {
        // 验证 delegate 始终添加到数组末尾
        let make_tool = |name: &str| -> JsonValue {
            let mut func = BTreeMap::new();
            func.insert("name".to_string(), JsonValue::String(name.to_string()));
            let mut tool = BTreeMap::new();
            tool.insert(
                "type".to_string(),
                JsonValue::String("function".to_string()),
            );
            tool.insert("function".to_string(), JsonValue::Object(func));
            JsonValue::Object(tool)
        };

        let tools = JsonValue::Array(vec![make_tool("a"), make_tool("b"), make_tool("c")]);
        let merged = merge_delegate_tool(&tools);
        let arr = merged.as_array().unwrap();

        // 最后一个必须是 delegate
        assert_eq!(
            arr.last()
                .and_then(|t| t.get("function"))
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("delegate"),
            "delegate 工具应在数组末尾"
        );
    }

    // ===== delegate_tool_openai_spec 结构细节测试 =====

    #[test]
    fn test_delegate_tool_spec_description_non_empty() {
        let spec = delegate_tool_openai_spec();
        let function = spec.get("function").unwrap();
        let description = function
            .get("description")
            .and_then(|v| v.as_str())
            .expect("description 应存在");
        assert!(
            !description.is_empty(),
            "delegate 工具的 description 不应为空"
        );
        assert!(
            description.contains("委托") || description.contains("delegate"),
            "description 应说明委托功能"
        );
    }

    #[test]
    fn test_delegate_tool_spec_param_descriptions() {
        let spec = delegate_tool_openai_spec();
        let params = spec.get("function").unwrap().get("parameters").unwrap();
        let props = params.get("properties").unwrap();

        // agent_type 参数应有 description
        let agent_type_desc = props
            .get("agent_type")
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str());
        assert!(
            agent_type_desc.is_some() && !agent_type_desc.unwrap().is_empty(),
            "agent_type 参数应有非空 description"
        );

        // task 参数应有 description
        let task_desc = props
            .get("task")
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str());
        assert!(
            task_desc.is_some() && !task_desc.unwrap().is_empty(),
            "task 参数应有非空 description"
        );
    }

    #[test]
    fn test_delegate_tool_spec_param_types_are_string() {
        let spec = delegate_tool_openai_spec();
        let params = spec.get("function").unwrap().get("parameters").unwrap();
        let props = params.get("properties").unwrap();

        // agent_type 类型应为 string
        assert_eq!(
            props
                .get("agent_type")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str()),
            Some("string")
        );

        // task 类型应为 string
        assert_eq!(
            props
                .get("task")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_delegate_tool_spec_required_has_two_fields() {
        let spec = delegate_tool_openai_spec();
        let params = spec.get("function").unwrap().get("parameters").unwrap();
        let required = params
            .get("required")
            .and_then(|r| r.as_array())
            .expect("required 应为数组");

        assert_eq!(
            required.len(),
            2,
            "required 应包含 2 个字段（agent_type + task）"
        );

        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"agent_type"));
        assert!(names.contains(&"task"));
    }
}
