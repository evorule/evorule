//! AgentRunner 集成测试
//!
//! 验证 AgentRunner 的 ReAct 循环端到端流程：
//! AgentRunner → Reactor → IoRequest → Mock Subscriber → IoResponse → Stable
//!
//! 使用 mock LLM 响应（不调用真实 LLM API）和真实 ToolHandler（echo 工具）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::{EventReceiver, Fact, FactId, FactSender, IoType, Reactor};
use tokio::sync::Mutex;

use tier2_governance::{
    io_handler::IoHandler, io_handlers::tool_handler::ToolHandler, AgentConfig, AgentError,
    AgentRunner, ToolRegistry,
};

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
    let core_eval_path = manifest_dir.join("../tier0-tcb/core_eval.json");

    let json_str = std::fs::read_to_string(&core_eval_path)
        .unwrap_or_else(|e| panic!("Failed to read core_eval.json: {}", e));

    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("Failed to parse core_eval.json");

    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

/// 构造 LLM 多轮模式响应 Object（LlmHandler 多轮模式返回格式）
fn make_llm_response(
    content: Option<&str>,
    tool_calls: Option<Vec<(&str, &str, &str)>>, // (id, name, arguments)
    finish_reason: &str,
) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert(
        "finish_reason".to_string(),
        JsonValue::String(finish_reason.to_string()),
    );
    match content {
        Some(c) => {
            m.insert("content".to_string(), JsonValue::String(c.to_string()));
        }
        None => {
            m.insert("content".to_string(), JsonValue::Null);
        }
    }
    match tool_calls {
        Some(tcs) => {
            let arr: Vec<JsonValue> = tcs
                .iter()
                .map(|(id, name, args)| {
                    let mut func = BTreeMap::new();
                    func.insert("name".to_string(), JsonValue::String(name.to_string()));
                    func.insert("arguments".to_string(), JsonValue::String(args.to_string()));
                    let mut tc = BTreeMap::new();
                    tc.insert("id".to_string(), JsonValue::String(id.to_string()));
                    tc.insert(
                        "type".to_string(),
                        JsonValue::String("function".to_string()),
                    );
                    tc.insert("function".to_string(), JsonValue::Object(func));
                    JsonValue::Object(tc)
                })
                .collect();
            m.insert("tool_calls".to_string(), JsonValue::Array(arr));
        }
        None => {
            m.insert("tool_calls".to_string(), JsonValue::Null);
        }
    }
    JsonValue::Object(m)
}

/// 构造 echo 工具的 JSON Schema
fn echo_schema() -> JsonValue {
    let mut props = BTreeMap::new();
    let mut text_param = BTreeMap::new();
    text_param.insert("type".to_string(), JsonValue::String("string".to_string()));
    text_param.insert(
        "description".to_string(),
        JsonValue::String("要回声的文本".to_string()),
    );
    props.insert("text".to_string(), JsonValue::Object(text_param));

    let mut schema = BTreeMap::new();
    schema.insert("type".to_string(), JsonValue::String("object".to_string()));
    schema.insert("properties".to_string(), JsonValue::Object(props));
    schema.insert(
        "required".to_string(),
        JsonValue::Array(vec![JsonValue::String("text".to_string())]),
    );
    JsonValue::Object(schema)
}

/// Mock I/O 订阅者：拦截 IoRequest，对 CallLlm 返回预设响应，对 CallTool 分发到 ToolHandler
///
/// 模拟 IoSubscriber 的行为，但不使用真实 LLM/DB/HTTP handler：
/// - CallLlm → 从预设响应队列中取出下一个响应
/// - CallTool → 调用 ToolHandler.execute() 执行真实工具
/// - 其他 IoType → 返回错误
async fn run_mock_subscriber(
    mut event_rx: EventReceiver,
    command_tx: FactSender,
    llm_responses: Arc<Mutex<Vec<JsonValue>>>,
    tool_handler: ToolHandler,
) {
    let mut next_id: u64 = 10000;
    loop {
        match event_rx.recv().await {
            Ok(Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            }) => {
                let response = match io_type {
                    IoType::CallLlm => {
                        let mut queue = llm_responses.lock().await;
                        if queue.is_empty() {
                            Fact::IoResponse {
                                id: FactId(next_id),
                                request_id: id,
                                result: JsonValue::Null,
                                error: Some("No more preset LLM responses".to_string()),
                            }
                        } else {
                            let resp = queue.remove(0);
                            Fact::IoResponse {
                                id: FactId(next_id),
                                request_id: id,
                                result: resp,
                                error: None,
                            }
                        }
                    }
                    IoType::CallTool => match tool_handler.execute(&params).await {
                        Ok(result) => Fact::IoResponse {
                            id: FactId(next_id),
                            request_id: id,
                            result,
                            error: None,
                        },
                        Err(err) => Fact::IoResponse {
                            id: FactId(next_id),
                            request_id: id,
                            result: JsonValue::Null,
                            error: Some(err),
                        },
                    },
                    _ => continue,
                };
                next_id += 1;
                let _ = command_tx.send(response);
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

/// 创建带 echo 工具的 ToolRegistry（供 AgentRunner 使用，生成工具描述）
fn create_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(
        "echo".to_string(),
        "回声工具：返回输入文本".to_string(),
        echo_schema(),
        Box::new(|args: &JsonValue| {
            let cloned = args.clone();
            Box::pin(async move {
                Ok(JsonValue::string(format!(
                    "echo: {}",
                    cloned.as_str().unwrap_or("(non-string)")
                )))
            })
        }),
    );
    registry
}

/// 创建带 echo 工具的 ToolHandler（供 mock subscriber 使用，执行工具调用）
fn create_tool_handler() -> ToolHandler {
    let mut handler = ToolHandler::new();
    handler.register(
        "echo".to_string(),
        Box::new(|args: &JsonValue| {
            let cloned = args.clone();
            Box::pin(async move {
                Ok(JsonValue::string(format!(
                    "echo: {}",
                    cloned.as_str().unwrap_or("(non-string)")
                )))
            })
        }),
    );
    handler
}

/// 搭建测试环境：Reactor + Mock Subscriber，返回 (AgentRunner, llm_responses)
fn setup_agent(
    core_eval: Vec<JsonValue>,
    llm_response_list: Vec<JsonValue>,
    max_steps: usize,
) -> (
    AgentRunner,
    Arc<Mutex<Vec<JsonValue>>>,
    tier1_reactor::ReactorHandle,
) {
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (command_tx, event_rx, event_tx, handle, _facts_log) = reactor.spawn();

    let llm_responses = Arc::new(Mutex::new(llm_response_list));

    // 启动 mock subscriber
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    let tool_handler = create_tool_handler();
    tokio::spawn(run_mock_subscriber(
        sub_rx,
        sub_tx,
        llm_responses.clone(),
        tool_handler,
    ));

    // 预计算工具描述（OpenAI tools 格式）
    let registry = create_tool_registry();
    let tools_json = registry.to_openai_tools(&["echo".to_string()]);

    let config = AgentConfig {
        system_prompt: "你是测试助手".to_string(),
        max_steps,
        step_timeout: Duration::from_secs(10),
        tool_names: vec!["echo".to_string()],
        ..Default::default()
    };
    let runner = AgentRunner::new(config, command_tx, event_rx, tools_json);

    (runner, llm_responses, handle)
}

// ===== 测试用例 =====

#[tokio::test]
async fn test_agent_single_step_finish() {
    // 场景：LLM 一步返回最终答案（finish_reason=stop）
    let core_eval = load_core_eval();
    let (mut runner, _llm_responses, _handle) = setup_agent(
        core_eval,
        vec![make_llm_response(Some("答案是 42"), None, "stop")],
        5,
    );

    let result = runner.run("生命、宇宙及一切的终极答案是什么？").await;

    assert!(result.is_ok(), "Agent should succeed: {:?}", result.err());
    let result = result.unwrap();
    assert_eq!(result.final_answer, "答案是 42");
    assert_eq!(result.steps, 1);
    // messages: [System, User, Assistant]
    assert_eq!(result.messages.len(), 3);
}

#[tokio::test]
async fn test_agent_multi_step_with_tool_call() {
    // 场景：LLM 先调用 echo 工具，再返回最终答案
    let core_eval = load_core_eval();
    let (mut runner, _llm_responses, _handle) = setup_agent(
        core_eval,
        vec![
            // 第一步：LLM 请求调用 echo 工具
            make_llm_response(
                None,
                Some(vec![("call_001", "echo", "hello")]),
                "tool_calls",
            ),
            // 第二步：LLM 收到工具结果后返回最终答案
            make_llm_response(Some("echo 返回了: echo: hello"), None, "stop"),
        ],
        5,
    );

    let result = runner.run("请用 echo 工具回声 hello").await;

    assert!(result.is_ok(), "Agent should succeed: {:?}", result.err());
    let result = result.unwrap();
    assert_eq!(result.final_answer, "echo 返回了: echo: hello");
    assert_eq!(result.steps, 2);
    // messages: [System, User, Assistant(tool_calls), Tool, Assistant]
    assert_eq!(result.messages.len(), 5);
}

#[tokio::test]
async fn test_agent_max_steps_exceeded() {
    // 场景：LLM 始终返回 tool_calls，永远不返回 stop → 超过 max_steps
    let core_eval = load_core_eval();
    let tool_call_ids: Vec<String> = (0..10).map(|i| format!("call_{}", i)).collect();
    let endless_tool_calls: Vec<JsonValue> = tool_call_ids
        .iter()
        .map(|id| {
            make_llm_response(
                None,
                Some(vec![(id.as_str(), "echo", "test")]),
                "tool_calls",
            )
        })
        .collect();
    let (mut runner, _llm_responses, _handle) = setup_agent(core_eval, endless_tool_calls, 2);

    let result = runner.run("无限循环测试").await;

    assert!(result.is_err(), "Agent should fail with MaxStepsExceeded");
    match result.err().unwrap() {
        AgentError::MaxStepsExceeded(n) => assert_eq!(n, 2),
        other => panic!("Expected MaxStepsExceeded, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_agent_empty_llm_response_queue() {
    // 场景：LLM 响应队列为空 → mock subscriber 返回错误 IoResponse
    // 反应器可能：发射 Error Fact、或 Stable（llm_response 缺失/为 Null）
    // AgentRunner 应正确处理两种情况
    let core_eval = load_core_eval();
    let (mut runner, _llm_responses, _handle) = setup_agent(core_eval, vec![], 5);

    let result = runner.run("测试空响应队列").await;

    match result {
        Ok(res) => {
            // 反应器将错误 IoResponse 转为空 llm_response → AgentRunner 返回空答案
            assert!(
                res.final_answer.is_empty(),
                "Expected empty final_answer, got: {}",
                res.final_answer
            );
        }
        Err(AgentError::MissingLlmResponse) | Err(AgentError::ReactorError(_)) => {
            // 反应器未设置 llm_response 或发射了 Error Fact
        }
        Err(other) => panic!("Unexpected error: {:?}", other),
    }
}
