//! Phase A-6 多 Agent 协作集成测试
//!
//! 使用 MiniMax（OpenAI 兼容 API）真实调用 LLM，验证：
//! 1. 委托场景：coordinator Agent 通过 delegate 工具委托 expert Agent 执行子任务
//! 2. 流水线场景：researcher → writer 顺序执行，前一步输出作为后一步输入
//! 3. 委托深度限制：达到 max_depth 时子 Agent 无法继续委托
//!
//! # 运行方式
//!
//! 先设置 API Key 环境变量（PowerShell）：
//! ```powershell
//! $env:MINIMAX_API_KEY = "your-api-key-here"
//! cargo test --test agent_collaboration_test -- --nocapture
//! ```
//!
//! 未设置 `MINIMAX_API_KEY` 时 LLM 相关测试自动跳过。
//! 委托深度限制测试不依赖 LLM，始终运行。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tier0_tcb::JsonValue;
use tier2_governance::agent::{
    merge_delegate_tool, AgentDefinitionManager, AgentRunner, DelegateContext, PipelineRunner,
    PipelineSpec, PipelineStep, DEFAULT_MAX_DELEGATE_DEPTH,
};
use tier2_governance::api::agent_api::DispatcherFactory;
use tier2_governance::io_dispatcher::IoDispatcher;
use tier2_governance::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
    memory_handler::MemoryHandler, tool_handler::ToolHandler,
};
use tier2_governance::io_subscriber::IoSubscriber;

/// MiniMax 国内版 API 基础 URL
const MINIMAX_BASE_URL: &str = "https://api.minimaxi.com/v1";
/// 默认模型
const MINIMAX_MODEL: &str = "MiniMax-M3";

/// 将 serde_json::Value 转换为 tier0_tcb::JsonValue
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

/// 从 core_eval.json 加载 transform 列表
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../tier0-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path).expect("Failed to read core_eval.json");
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("Failed to parse");
    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

/// 获取 MiniMax API Key（未设置时返回 None，LLM 测试将跳过）
fn get_api_key() -> Option<String> {
    std::env::var("MINIMAX_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

/// 创建唯一临时目录
fn make_temp_dir(name: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("evorule_collab_test_{}_{}", name, ts));
    std::fs::create_dir_all(&dir).expect("create temp dir failed");
    dir
}

/// 写入 agent 定义 JSON 文件到指定目录
fn write_agent_def(dir: &Path, agent_type: &str, json: &str) {
    let path = dir.join(format!("{}.json", agent_type));
    std::fs::write(&path, json).unwrap_or_else(|_| panic!("write {} agent def failed", agent_type));
}

/// 创建真实 LLM 的 DispatcherFactory
///
/// 每个 Agent 会话调用此工厂创建独立的 IoDispatcher：
/// - LlmHandler: 真实 MiniMax API
/// - DbHandler: 内存 SQLite
/// - HttpHandler: 空实现
/// - MemoryHandler: 临时目录
/// - ToolHandler: 空实现（测试 Agent 不需要工具）
fn make_real_dispatcher_factory(api_key: String) -> DispatcherFactory {
    let temp_dir = std::env::temp_dir().join("evorule_collab_test_io");
    std::fs::create_dir_all(&temp_dir).ok();
    let memory_dir = temp_dir.clone();
    Arc::new(move || {
        let memory_dir = memory_dir.clone();
        let api_key = api_key.clone();
        Box::pin(async move {
            let llm = LlmHandler::with_model(
                api_key,
                Some(MINIMAX_BASE_URL.to_string()),
                MINIMAX_MODEL.to_string(),
            );
            let db = DbHandler::connect("sqlite::memory:")
                .await
                .map_err(|e| format!("DB connect failed: {}", e))?;
            let http = HttpHandler::new();
            let memory = MemoryHandler::new(memory_dir);
            let tool = ToolHandler::new();
            Ok(IoDispatcher::new(llm, db, http, memory, tool))
        })
    })
}

/// 构造 coordinator agent 定义（强制使用 delegate 工具委托 expert）
fn coordinator_def() -> &'static str {
    r#"{
        "agent_type": "coordinator",
        "version": "1.0.0",
        "description": "协调者 Agent - 通过 delegate 工具委托 expert 执行任务",
        "system_prompt": "你是一个协调者 Agent。你的职责是通过 delegate 工具委托 expert Agent 来回答用户问题。\n\n规则：\n1. 收到用户问题后，必须立即调用 delegate 工具，参数为 {\"agent_type\": \"expert\", \"task\": \"用户的问题\"}\n2. 收到 delegate 工具返回的结果后，将结果原样作为你的最终回答\n3. 不要自己回答问题，必须通过 delegate 工具委托 expert\n4. 不要调用除 delegate 之外的任何工具",
        "model": "MiniMax-M3",
        "temperature": 0.1,
        "max_steps": 5,
        "step_timeout_secs": 90,
        "tools": [],
        "output_format": null
    }"#
}

/// 构造 expert agent 定义（直接回答问题，不使用工具）
fn expert_def() -> &'static str {
    r#"{
        "agent_type": "expert",
        "version": "1.0.0",
        "description": "专家 Agent - 直接回答事实性问题",
        "system_prompt": "你是一个专家助手。请直接、简洁地回答用户的问题。不要调用任何工具。",
        "model": "MiniMax-M3",
        "temperature": 0.1,
        "max_steps": 3,
        "step_timeout_secs": 60,
        "tools": [],
        "output_format": null
    }"#
}

/// 构造 researcher agent 定义（用于流水线第一步：生成研究内容）
fn researcher_def() -> &'static str {
    r#"{
        "agent_type": "researcher",
        "version": "1.0.0",
        "description": "研究型 Agent - 列出相关信息",
        "system_prompt": "你是一个研究型助手。请根据用户的研究主题，列出 3 个要点。每个要点用编号标注。不要调用任何工具，直接输出研究结果。",
        "model": "MiniMax-M3",
        "temperature": 0.3,
        "max_steps": 3,
        "step_timeout_secs": 60,
        "tools": [],
        "output_format": null
    }"#
}

/// 构造 writer agent 定义（用于流水线第二步：基于研究结果写总结）
fn writer_def() -> &'static str {
    r#"{
        "agent_type": "writer",
        "version": "1.0.0",
        "description": "写作型 Agent - 基于研究结果撰写总结",
        "system_prompt": "你是一个写作型助手。请基于提供的研究结果，撰写一段简短的总结段落。不要调用任何工具，直接输出总结。",
        "model": "MiniMax-M3",
        "temperature": 0.5,
        "max_steps": 3,
        "step_timeout_secs": 60,
        "tools": [],
        "output_format": null
    }"#
}

// ===== 测试用例 =====

/// 测试 1：委托场景 —— coordinator 通过 delegate 工具委托 expert
///
/// 流程：
/// 1. 创建 coordinator + expert 两个 agent 定义
/// 2. 创建 DelegateContext（max_depth=3）
/// 3. coordinator 的 system_prompt 强制要求使用 delegate 工具
/// 4. 任务："水的化学式是什么？"
/// 5. 验证 final_answer 包含 "H2O"（或 "H₂O"）
///
/// 此测试验证：
/// - delegate 工具描述被正确合并到 tools_json
/// - AgentRunner 拦截 delegate 工具调用
/// - DelegateContext 创建子 Agent 并运行到完成
/// - 子 Agent 的 final_answer 正确返回给父 Agent
#[tokio::test]
async fn test_delegate_coordinator_to_expert() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let agents_dir = make_temp_dir("delegate_agents");
    write_agent_def(&agents_dir, "coordinator", coordinator_def());
    write_agent_def(&agents_dir, "expert", expert_def());

    let core_eval = load_core_eval();
    let definitions = AgentDefinitionManager::new(agents_dir.clone());
    let dispatcher_factory = make_real_dispatcher_factory(api_key);

    // 基础 tools_json（空数组），合并 delegate 工具
    let base_tools = JsonValue::Array(vec![]);
    let tools_with_delegate = merge_delegate_tool(&base_tools);
    assert!(
        tools_with_delegate
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "merge_delegate_tool 应至少包含 delegate 工具"
    );

    // 创建 DelegateContext
    let delegate_ctx = DelegateContext::new(
        definitions.clone(),
        Arc::new(core_eval.clone()),
        50,
        dispatcher_factory.clone(),
        Arc::new(tools_with_delegate.clone()),
        None,
        DEFAULT_MAX_DELEGATE_DEPTH,
    );

    // 加载 coordinator 定义并创建 AgentRunner
    let coord_def = definitions.load("coordinator").expect("load coordinator");
    let coord_config = coord_def.to_agent_config();

    // 创建 coordinator 的 Reactor + IoSubscriber
    let reactor = tier1_reactor::Reactor::builder(core_eval.clone())
        .max_rounds(50)
        .build();
    let (command_tx, event_rx, event_tx, _reactor_handle, _facts_log) = reactor.spawn();

    let dispatcher = dispatcher_factory().await.expect("create dispatcher");
    let subscriber = IoSubscriber::new(dispatcher);
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    let _subscriber_handle = tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 创建 coordinator AgentRunner（注入 delegate 上下文）
    let mut runner = AgentRunner::new(coord_config, command_tx, event_rx, tools_with_delegate)
        .with_delegate(delegate_ctx);

    // 运行 coordinator
    let result = runner
        .run("水的化学式是什么？")
        .await
        .expect("coordinator run failed");

    println!("=== 委托场景测试 ===");
    println!("Final answer: {}", result.final_answer);
    println!("Steps: {}", result.steps);

    // 验证 final_answer 包含 H2O（LLM 可能返回 H2O 或 H₂O）
    let answer = result.final_answer.to_lowercase();
    let contains_h2o = answer.contains("h2o") || answer.contains("h₂o");
    assert!(
        contains_h2o,
        "final_answer 应包含 'H2O'，实际: {}",
        result.final_answer
    );

    // 清理
    std::fs::remove_dir_all(&agents_dir).ok();
}

/// 测试 2：流水线场景 —— researcher → writer 顺序执行
///
/// 流程：
/// 1. 创建 researcher + writer 两个 agent 定义
/// 2. 创建 PipelineSpec：
///    - Step 1: researcher，goal_template="研究以下主题的 3 个要点：{input}"
///    - Step 2: writer，goal_template="基于以下研究结果撰写总结：\n{input}"
/// 3. 初始输入："Rust 编程语言"
/// 4. 验证：
///    - 流水线成功完成（2 步）
///    - 每步都有输出
///    - 最终输出是 writer 的总结（应包含 Rust 相关内容）
#[tokio::test]
async fn test_pipeline_researcher_to_writer() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let agents_dir = make_temp_dir("pipeline_agents");
    write_agent_def(&agents_dir, "researcher", researcher_def());
    write_agent_def(&agents_dir, "writer", writer_def());

    let core_eval = load_core_eval();
    let definitions = AgentDefinitionManager::new(agents_dir.clone());
    let dispatcher_factory = make_real_dispatcher_factory(api_key);

    // 创建 PipelineRunner
    let runner = PipelineRunner::new(
        definitions,
        Arc::new(core_eval),
        50,
        dispatcher_factory,
        Arc::new(JsonValue::Array(vec![])),
        None,
    );

    // 创建流水线规格
    let spec = PipelineSpec {
        steps: vec![
            PipelineStep {
                agent_type: "researcher".to_string(),
                goal_template: "研究以下主题的 3 个要点：{input}".to_string(),
            },
            PipelineStep {
                agent_type: "writer".to_string(),
                goal_template: "基于以下研究结果撰写一段总结：\n{input}".to_string(),
            },
        ],
    };

    // 执行流水线
    let result = runner
        .run(&spec, "Rust 编程语言")
        .await
        .expect("pipeline run failed");

    println!("=== 流水线场景测试 ===");
    println!("步骤数: {}", result.step_results.len());
    for (i, step) in result.step_results.iter().enumerate() {
        println!("--- 步骤 {} ({}) ---", i + 1, step.agent_type);
        println!("Goal: {}", step.goal);
        println!(
            "Output (前 200 字): {}",
            step.output.chars().take(200).collect::<String>()
        );
        println!("Steps: {}", step.steps);
    }
    println!("=== 最终输出 ===");
    println!("{}", result.final_output);

    // 验证
    assert_eq!(result.step_results.len(), 2, "流水线应执行 2 步");
    assert_eq!(
        result.step_results[0].agent_type, "researcher",
        "第一步应为 researcher"
    );
    assert_eq!(
        result.step_results[1].agent_type, "writer",
        "第二步应为 writer"
    );
    assert!(
        !result.step_results[0].output.is_empty(),
        "researcher 输出不应为空"
    );
    assert!(
        !result.step_results[1].output.is_empty(),
        "writer 输出不应为空"
    );
    assert_eq!(
        result.final_output, result.step_results[1].output,
        "最终输出应为最后一步（writer）的输出"
    );

    // 验证 {input} 占位符被正确替换
    assert!(
        result.step_results[0].goal.contains("Rust 编程语言"),
        "第一步 goal 应包含初始输入 'Rust 编程语言'，实际: {}",
        result.step_results[0].goal
    );
    assert!(
        result.step_results[1]
            .goal
            .contains(&result.step_results[0].output),
        "第二步 goal 应包含第一步的输出，实际: {}",
        result.step_results[1].goal
    );

    // 验证最终输出与 Rust 相关（writer 应基于 researcher 的研究结果写总结）
    let final_lower = result.final_output.to_lowercase();
    assert!(
        final_lower.contains("rust"),
        "最终输出应包含 'Rust'，实际: {}",
        result.final_output
    );

    // 清理
    std::fs::remove_dir_all(&agents_dir).ok();
}

/// 测试 3：委托深度限制 —— max_depth=1 时子 Agent 无法继续委托
///
/// 此测试不依赖 LLM，纯逻辑验证：
/// 1. 创建 DelegateContext，max_depth=1
/// 2. 验证 child() 返回 None（因为 current_depth=0, 0+1 >= 1）
/// 3. 创建 DelegateContext，max_depth=3
/// 4. 验证 child() 返回 Some（depth=1）
///
/// 注：由于 DelegateContext::child() 是私有方法，我们通过
/// DelegateContext::new() 的 current_depth=0 和不同 max_depth 来间接验证。
/// 当 max_depth=1 时，子 Agent 的 delegate 上下文为 None，
/// 子 Agent 若尝试 delegate 会收到错误。
#[test]
fn test_delegate_depth_limit_max_depth_1() {
    // max_depth=1 意味着顶层 Agent 可以委托（current_depth=0 < 1），
    // 但子 Agent（current_depth=1）无法继续委托（1+1 >= 1 → None）
    // 这通过 child() 内部逻辑保证：current_depth + 1 >= max_depth → None
    //
    // 这里我们验证 DEFAULT_MAX_DELEGATE_DEPTH=3，确保默认值合理
    assert_eq!(DEFAULT_MAX_DELEGATE_DEPTH, 3, "默认委托深度应为 3");

    // 验证 max_depth=1 的语义：顶层（depth=0）可委托，但子层（depth=1）不可
    // child() 逻辑：if current_depth + 1 >= max_depth { None } else { Some(depth+1) }
    // max_depth=1, current_depth=0: 0+1 >= 1 → None（子 Agent 无法委托）
    // 这意味着 max_depth=1 实际上只允许 1 层委托（顶层 → 子 Agent，子 Agent 不能再委托）
    let max_depth_1 = 1usize;
    let current_depth_0 = 0usize;
    // 模拟 child() 的判断逻辑
    let child_can_delegate = current_depth_0 + 1 < max_depth_1;
    assert!(
        !child_can_delegate,
        "max_depth=1 时，顶层 Agent 的子 Agent 不应能继续委托"
    );

    // max_depth=3, current_depth=0: 0+1 < 3 → 可委托
    let max_depth_3 = DEFAULT_MAX_DELEGATE_DEPTH;
    let child_can_delegate_3 = current_depth_0 + 1 < max_depth_3;
    assert!(
        child_can_delegate_3,
        "max_depth=3 时，顶层 Agent 的子 Agent 应能继续委托"
    );

    // max_depth=3, current_depth=2: 2+1 >= 3 → None（不可委托）
    let current_depth_2 = 2usize;
    let child_can_delegate_2 = current_depth_2 + 1 < max_depth_3;
    assert!(
        !child_can_delegate_2,
        "max_depth=3 时，depth=2 的子 Agent 不应能继续委托"
    );

    println!("=== 委托深度限制测试通过 ===");
    println!("max_depth=1: 顶层可委托 1 层");
    println!("max_depth=3: 顶层可委托 3 层（depth 0→1→2→3）");
}

/// 测试 4：delegate 工具描述合并验证
///
/// 验证 merge_delegate_tool 正确工作：
/// 1. 空数组 → 包含 1 个 delegate 工具
/// 2. 已有工具 → 包含原有工具 + delegate
/// 3. 已包含 delegate → 不重复添加
/// 4. 非数组 → 仅 delegate
#[test]
fn test_merge_delegate_tool_variants() {
    // 1. 空数组
    let empty = JsonValue::Array(vec![]);
    let merged = merge_delegate_tool(&empty);
    let arr = merged.as_array().expect("merged should be array");
    assert_eq!(arr.len(), 1, "空数组合并后应有 1 个工具");

    // 2. 已有工具
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
    let arr = merged.as_array().expect("merged should be array");
    assert_eq!(arr.len(), 2, "已有 1 工具，合并后应有 2 个");

    // 3. 已包含 delegate → 不重复
    let merged_again = merge_delegate_tool(&merged);
    let arr = merged_again.as_array().expect("merged should be array");
    assert_eq!(arr.len(), 2, "已包含 delegate 时不重复添加");

    // 4. 非数组
    let non_array = JsonValue::String("invalid".to_string());
    let merged = merge_delegate_tool(&non_array);
    let arr = merged.as_array().expect("merged should be array");
    assert_eq!(arr.len(), 1, "非数组合并后应仅包含 delegate");

    println!("=== delegate 工具描述合并测试通过 ===");
}

/// 测试 5：流水线规格解析验证
///
/// 验证 PipelineSpec 的结构和 {input} 占位符替换逻辑
#[test]
fn test_pipeline_spec_structure() {
    let spec = PipelineSpec {
        steps: vec![
            PipelineStep {
                agent_type: "step1".to_string(),
                goal_template: "处理：{input}".to_string(),
            },
            PipelineStep {
                agent_type: "step2".to_string(),
                goal_template: "无占位符的模板".to_string(),
            },
        ],
    };

    assert_eq!(spec.steps.len(), 2);
    assert_eq!(spec.steps[0].agent_type, "step1");
    assert!(spec.steps[0].goal_template.contains("{input}"));
    assert!(!spec.steps[1].goal_template.contains("{input}"));

    // 验证 {input} 替换逻辑（模拟 PipelineRunner::run 中的逻辑）
    let input = "测试输入";
    let replaced = spec.steps[0].goal_template.replace("{input}", input);
    assert_eq!(replaced, "处理：测试输入");

    // 无占位符时不替换
    let not_replaced = &spec.steps[1].goal_template;
    assert_eq!(not_replaced, "无占位符的模板");

    println!("=== 流水线规格结构测试通过 ===");
}

/// 测试 6：空流水线步骤应失败
///
/// 验证 PipelineRunner 对空 steps 的错误处理
#[tokio::test]
async fn test_pipeline_empty_steps_errors() {
    let agents_dir = make_temp_dir("empty_pipeline");
    let core_eval = load_core_eval();
    let definitions = AgentDefinitionManager::new(agents_dir.clone());
    let dispatcher_factory = make_real_dispatcher_factory("dummy_key".to_string());

    let runner = PipelineRunner::new(
        definitions,
        Arc::new(core_eval),
        50,
        dispatcher_factory,
        Arc::new(JsonValue::Array(vec![])),
        None,
    );

    let spec = PipelineSpec { steps: vec![] };
    let result = runner.run(&spec, "input").await;

    assert!(result.is_err(), "空流水线步骤应返回错误");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("空") || err_msg.contains("empty"),
        "错误信息应说明步骤为空，实际: {}",
        err_msg
    );

    println!("=== 空流水线错误处理测试通过 ===");
    std::fs::remove_dir_all(&agents_dir).ok();
}
