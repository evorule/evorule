// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! # reactive_researcher — EvoRule Reference 实现(1.0 §4.4 门槛)
//!
//! 端到端演示三层架构(evorule-tcb + evorule-reactor + evorule-governance)的完整用法。
//!
//! ## 工作流
//! 1. 提交 `call_external` Command(LLM 分析)→ TCB 发 `IoRequest` →
//!    自定义 `LlmHandler` 处理 → 回写 `IoResponse` → TCB 注入 `payload.llm_response` → Stable
//! 2. 提交 `save_memory` Command(持久化)→ TCB 发 `IoRequest` →
//!    复用 `evorule_governance::MemoryHandler` → 回写 `IoResponse` → Stable
//! 3. 打印 `FactsLog::history()` 完整审计链
//!
//! ## 设计要点
//! - **不修改任何核心 crate**:所有自定义代码(LlmHandler / ExampleSubscriber)在本包内
//! - **绕过 `IoDispatcher`**:核心 dispatcher 强制要 DbHandler(SQLite),且 call_external 被错路由
//!   到 HttpHandler(期望 URL 而非 LLM 参数);本例直接按 io_type 分发到合适的 handler
//! - **dry-run 默认模式**:无需网络/API key 即可重复运行,展示确定性 canned 响应
//! - **live 模式可选**:通过 `--llm-mode live --llm-url ... --llm-api-key ...` 调用真实 LLM API

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use clap::Parser;
use evorule_tcb::JsonValue;
use evorule_reactor::serde_to_tcb;
use evorule_reactor::{
    EventReceiver, Fact, FactId, FactIdGenerator, FactSender, FactsLog, IoHandler, IoResult,
    IoType, Reactor,
};
use tokio::time::timeout;

// ============================================================================
// H5: MemoryHandler 内联实现
// ============================================================================
// H5 + 走神 9: MemoryHandler 已两次外迁，最终位于 evorule-server 独立仓 core/io_handlers/
// 此 example 属于核心 workspace,不能依赖应用层 crate,故内联简单实现。
// 生产环境请使用 evorule-server 独立仓中 evorule_io_handlers::MemoryHandler。

/// 文件系统键值存储(示例用,简化版)
struct MemoryHandler {
    base_dir: PathBuf,
}

impl MemoryHandler {
    fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn resolve_path(&self, key: &str) -> PathBuf {
        let safe_key = key.replace(['/', '\\'], "_").replace("..", "_");
        self.base_dir.join(safe_key)
    }
}

#[async_trait]
impl IoHandler for MemoryHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: key".to_string())?;
        let path = self.resolve_path(key);

        if let Some(value) = params.get("value") {
            let content = value
                .as_str()
                .ok_or_else(|| "param 'value' must be a string".to_string())?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("create dir failed: {e}"))?;
            }
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| format!("write file failed: {e}"))?;
            Ok(JsonValue::Bool(true))
        } else {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("read file failed: {e}"))?;
            Ok(JsonValue::String(content))
        }
    }
}

// ============================================================================
// 段 1:CLI 定义
// ============================================================================

/// 命令行参数
#[derive(Debug, Clone, Parser)]
#[command(
    name = "reactive_researcher",
    about = "EvoRule reference implementation — reactive researcher demo (1.0 §4.4 gate)"
)]
struct Cli {
    /// core_eval.json 路径(默认指向仓库根的 evorule-tcb/core_eval.json)
    #[arg(
        long,
        env = "EVORULE_CORE_EVAL",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../evorule-tcb/core_eval.json")
    )]
    core_eval: PathBuf,

    /// Memory 持久化目录
    #[arg(
        long,
        env = "EVORULE_MEMORY_DIR",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/reactive_researcher_memory")
    )]
    memory_dir: PathBuf,

    /// LLM 模式:dry-run(默认,无网络)或 live(调用真实 LLM API)
    #[arg(long, env = "EVORULE_LLM_MODE", default_value = "dry-run")]
    llm_mode: LlmMode,

    /// live 模式下的 LLM API URL(OpenAI 兼容端点)
    #[arg(long, env = "EVORULE_LLM_URL")]
    llm_url: Option<String>,

    /// live 模式下的 API key
    #[arg(long, env = "EVORULE_LLM_API_KEY")]
    llm_api_key: Option<String>,

    /// 待研究的主题(将作为 prompt 发给 LLM)
    #[arg(long, default_value = "请用三句话总结 EvoRule 框架的设计哲学")]
    topic: String,

    /// Memory key(保存 LLM 响应的键名)
    #[arg(long, default_value = "research_note_001")]
    memory_key: String,
}

/// LLM 运行模式
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum LlmMode {
    /// 干跑模式:返回确定性 canned 响应,无需网络
    DryRun,
    /// 实模式:调用真实 LLM API
    Live,
}

// ============================================================================
// 段 2:LlmHandler — 自定义 IoHandler 实现
// ============================================================================

/// LLM 处理器
///
/// 实现 `IoHandler` trait,处理 `call_external` 类型的 I/O 请求。
/// - `DryRun`:返回确定性 canned 响应(默认,无网络)
/// - `Live`:通过 reqwest 调用 OpenAI 兼容的 chat completions API
struct LlmHandler {
    mode: LlmMode,
    client: reqwest::Client,
    llm_url: Option<String>,
    llm_api_key: Option<String>,
}

impl LlmHandler {
    /// 从 CLI 参数构造 LLM handler
    fn new(cli: &Cli) -> Self {
        Self {
            mode: cli.llm_mode,
            client: reqwest::Client::new(),
            llm_url: cli.llm_url.clone(),
            llm_api_key: cli.llm_api_key.clone(),
        }
    }
}

#[async_trait]
impl IoHandler for LlmHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 prompt(必需)
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: prompt".to_string())?;

        match self.mode {
            LlmMode::DryRun => {
                // 确定性 canned 响应:无网络,可重复运行
                let canned = format!(
                    "[dry-run LLM] 关于 '{prompt}' 的研究结论: \
                     EvoRule 通过三层架构(tier0 TCB / tier1 reactor / tier2 governance) \
                     实现事实驱动的确定性执行。TCB 保持 no_std + forbid(unsafe_code), \
                     所有 I/O 经 Fact 通道异步外挂,核心可形式化验证。"
                );
                Ok(JsonValue::String(canned))
            }
            LlmMode::Live => {
                let url = self
                    .llm_url
                    .as_ref()
                    .ok_or_else(|| "live mode requires --llm-url or EVORULE_LLM_URL".to_string())?;

                // 从 params 提取可选字段(model / system)
                let model = params
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gpt-3.5-turbo");
                let system = params.get("system").and_then(|v| v.as_str()).unwrap_or("");

                // 构造 OpenAI 兼容 chat completions 请求体
                let body = serde_json::json!({
                    "model": model,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": prompt},
                    ],
                });

                let resp = self
                    .client
                    .post(url)
                    .bearer_auth(self.llm_api_key.as_deref().unwrap_or(""))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| format!("LLM request failed: {e}"))?;

                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(format!("LLM API returned {status}: {body}"));
                }

                let json: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("LLM response parse failed: {e}"))?;

                let text = json["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("(empty response)")
                    .to_string();
                Ok(JsonValue::String(text))
            }
        }
    }
}

// ============================================================================
// 段 3:ExampleSubscriber — 自定义 I/O 分发循环
// ============================================================================

/// I/O 订阅者 ID 起始偏移
///
/// 与核心 `evorule_governance::IoSubscriber` 一致,从 10000 起,
/// 避免与反应器自身的 `FactIdGenerator`(从 1 起)冲突。
const SUBSCRIBER_ID_OFFSET: u64 = 10000;

/// 自定义 I/O 订阅者
///
/// 订阅反应器的 event broadcast 通道,过滤出 `Fact::IoRequest`,
/// 按 `io_type` 分发到对应的 handler,再通过 command 通道回写 `Fact::IoResponse`。
///
/// 与核心 `IoSubscriber` 的区别:
/// - 不依赖 `IoDispatcher`(后者强制要 DbHandler)
/// - 只处理 demo 用到的两种 io_type(call_external / save_memory)
struct ExampleSubscriber {
    llm: LlmHandler,
    memory: MemoryHandler,
    next_id: u64,
}

impl ExampleSubscriber {
    fn new(llm: LlmHandler, memory: MemoryHandler) -> Self {
        Self {
            llm,
            memory,
            next_id: SUBSCRIBER_ID_OFFSET,
        }
    }

    /// 生成下一个 FactId 并推进计数器
    fn next_fact_id(&mut self) -> FactId {
        let id = FactId(self.next_id);
        self.next_id += 1;
        id
    }

    /// 启动订阅循环
    ///
    /// - 接收 `Fact::IoRequest` → 调度执行 → 回写 `Fact::IoResponse`
    /// - 忽略其他 Fact 类型(由 main 任务消费)
    /// - `Lagged` 容错继续,`Closed` 正常退出
    async fn run(mut self, mut event_rx: EventReceiver, command_tx: FactSender) {
        tracing::info!(
            id_offset = SUBSCRIBER_ID_OFFSET,
            "ExampleSubscriber 启动,开始订阅 event broadcast 通道"
        );

        loop {
            match event_rx.recv().await {
                Ok(fact) => {
                    if let Fact::IoRequest {
                        id,
                        io_type,
                        params,
                        ..
                    } = fact
                    {
                        self.dispatch_and_respond(id, io_type, params, &command_tx)
                            .await;
                    }
                    // 忽略其他 Fact 类型
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "ExampleSubscriber 落后 {n} 条 Fact,已跳过");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Event 通道已关闭,ExampleSubscriber 退出");
                    return;
                }
            }
        }
    }

    /// 执行 I/O 调度并回写 `IoResponse`
    ///
    /// - 成功:`result = JsonValue`,`error = None`
    /// - 失败:`result = JsonValue::Null`,`error = Some(msg)`
    async fn dispatch_and_respond(
        &mut self,
        request_id: FactId,
        io_type: IoType,
        params: JsonValue,
        command_tx: &FactSender,
    ) {
        tracing::info!(
            %request_id,
            %io_type,
            "ExampleSubscriber 处理 IoRequest"
        );

        let result: IoResult = match io_type.as_str() {
            "call_external" => self.llm.execute(&params).await,
            "save_memory" => self.memory.execute(&params).await,
            other => Err(format!(
                "unsupported io_type in demo: {other} (only call_external and save_memory are handled)"
            )),
        };

        let (result_value, error) = match result {
            Ok(v) => (v, None),
            Err(e) => {
                tracing::warn!(%request_id, error = %e, "I/O 执行失败");
                (JsonValue::Null, Some(e))
            }
        };

        let response = Fact::IoResponse {
            id: self.next_fact_id(),
            request_id,
            result: result_value,
            error,
        };

        if command_tx.send(response).is_err() {
            tracing::warn!(%request_id, "command 通道已关闭,反应器已退出");
        }
    }
}

// ============================================================================
// 段 4:main 主流程
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    println!("═══════════════════════════════════════════════════════════════");
    println!("  reactive_researcher — EvoRule Reference 实现");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("配置:");
    println!("  core_eval  = {}", cli.core_eval.display());
    println!("  memory_dir = {}", cli.memory_dir.display());
    println!("  llm_mode   = {:?}", cli.llm_mode);
    println!("  topic      = {}", cli.topic);
    println!("  memory_key = {}", cli.memory_key);
    println!();

    // 1. 加载 core_eval.json
    let core_eval = load_core_eval(&cli.core_eval)?;
    tracing::info!(rules_count = core_eval.len(), "core_eval.json 加载完成");

    // 2. 创建 handlers
    std::fs::create_dir_all(&cli.memory_dir)?;
    let llm = LlmHandler::new(&cli);
    let memory = MemoryHandler::new(cli.memory_dir.clone());

    // 3. 构建并 spawn 反应器
    let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    // 4. spawn 自定义 subscriber(用 event_tx.subscribe() 创建独立接收端)
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        ExampleSubscriber::new(llm, memory)
            .run(sub_rx, sub_tx)
            .await;
    });

    let mut gen = FactIdGenerator::new();

    // ─────────────────────────────────────────────────────────────
    // 5. Command 1:call_external(LLM 分析)
    // ─────────────────────────────────────────────────────────────
    println!("┌─ 步骤 1:call_external(LLM 分析)─────────────────────────────");
    let cmd1 = Fact::Command {
        id: gen.next_id(),
        instruction: make_call_external_instruction(&cli.topic),
    };
    command_tx.send(cmd1)?;
    let snapshot1 = wait_for_stable(&mut event_rx).await?;

    // 6. 提取 LLM 响应
    let llm_response = snapshot1
        .get("llm_response")
        .and_then(|v| v.as_str())
        .ok_or("llm_response 字段缺失:call_external 未完成或结果未注入")?
        .to_string();
    println!("│ LLM 响应:");
    println!("│   {llm_response}");
    println!("└──────────────────────────────────────────────────────────────");
    println!();

    // ─────────────────────────────────────────────────────────────
    // 7. Command 2:save_memory(持久化)
    // ─────────────────────────────────────────────────────────────
    println!("┌─ 步骤 2:save_memory(持久化到 {}) ─────", cli.memory_key);
    let cmd2 = Fact::Command {
        id: gen.next_id(),
        instruction: make_save_memory_instruction(&cli.memory_key, &llm_response),
    };
    command_tx.send(cmd2)?;
    let snapshot2 = wait_for_stable(&mut event_rx).await?;

    // 8. 验证 memory_result
    let memory_ok = snapshot2.get("memory_result").and_then(|v| v.as_bool()) == Some(true);
    println!("│ memory_result = {memory_ok}");
    if memory_ok {
        println!(
            "│ 文件已写入:{}/{}",
            cli.memory_dir.display(),
            cli.memory_key
        );
    }
    println!("└──────────────────────────────────────────────────────────────");
    println!();

    // 9. 打印审计链
    print_audit_chain(&facts_log);

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("  ✓ Reference 实现运行完成");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

// ============================================================================
// 段 5:辅助函数
// ============================================================================

/// 从文件加载 core_eval.json,返回 transform 规则列表
///
/// 复用 `evorule_reactor::wal::serde_to_tcb` 把 `serde_json::Value` 转为
/// `evorule_tcb::JsonValue`(evorule-tcb 是 no_std crate,未实现 serde)。
fn load_core_eval(path: &PathBuf) -> Result<Vec<JsonValue>, Box<dyn std::error::Error>> {
    let json_str = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 core_eval.json 失败 ({}): {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("解析 core_eval.json 失败: {e}"))?;
    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .ok_or("core_eval.json 缺少 transform 数组")?;
    let core_eval: Vec<JsonValue> = transform.iter().map(serde_to_tcb).collect();
    Ok(core_eval)
}

/// 构造 `call_external` 指令
fn make_call_external_instruction(prompt: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("prompt".to_string(), JsonValue::string(prompt));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 `save_memory` 指令
fn make_save_memory_instruction(key: &str, value: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("key".to_string(), JsonValue::string(key));
    params.insert("value".to_string(), JsonValue::string(value));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("save_memory"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 等待反应器稳定,返回 final_snapshot(payload)
///
/// 30 秒超时,期间忽略 StateTransition / IoRequest / IoResponse 等 Fact,
/// 仅在 Stable 或 Error 时返回。
async fn wait_for_stable(
    event_rx: &mut EventReceiver,
) -> Result<JsonValue, Box<dyn std::error::Error>> {
    timeout(Duration::from_secs(30), async {
        loop {
            match event_rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return Ok(final_snapshot),
                Ok(Fact::Error { message, .. }) => {
                    return Err(format!("Reactor error: {message}").into())
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(lagged = n, "main event_rx 落后 {n} 条 Fact");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("Event channel closed unexpectedly".into())
                }
            }
        }
    })
    .await
    .map_err(|_| -> Box<dyn std::error::Error> { "Timeout waiting for Stable (30s)".into() })?
}

/// 打印完整审计链
///
/// 遍历 `FactsLog::history()`,逐条打印 Fact 的 ID、类型与关键信息,
/// 展示事实驱动的可审计性。
fn print_audit_chain(facts_log: &FactsLog) {
    let history = facts_log.history();
    println!(
        "┌─ 审计链(FactsLog::history,共 {} 条 Fact)─────────────",
        history.len()
    );
    for fact in &history {
        match fact {
            Fact::Command { id, instruction } => {
                let instr_type = instruction
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                println!("│ [{id:>6}] Command         instruction.type = {instr_type}");
            }
            Fact::PayloadUpdate { id, path, .. } => {
                println!("│ [{id:>6}] PayloadUpdate   path = {path}");
            }
            Fact::StateTransition { id, cause, .. } => {
                println!("│ [{id:>6}] StateTransition cause = {cause}");
            }
            Fact::IoRequest {
                id, cause, io_type, ..
            } => {
                println!("│ [{id:>6}] IoRequest       io_type = {io_type}, cause = {cause}");
            }
            Fact::IoResponse {
                id,
                request_id,
                error,
                ..
            } => {
                let status = if error.is_some() { "FAIL" } else { "OK" };
                println!(
                    "│ [{id:>6}] IoResponse      request_id = {request_id}, status = {status}"
                );
            }
            Fact::Stable { id, .. } => {
                println!("│ [{id:>6}] Stable          (reactor reached stable state)");
            }
            Fact::Error { id, message } => {
                println!("│ [{id:>6}] Error           message = {message}");
            }
        }
    }
    println!("└──────────────────────────────────────────────────────────────");
}
