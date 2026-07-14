//! evorule-server —— 独立二进制服务入口
//!
//! 启动 GovernanceServer（HTTP API + SSE 事件流 + 多会话管理），
//! 内置 IoSubscriber（LLM / DB / HTTP / Memory / Tool 五种 I/O handler）。
//!
//! # 用法
//! ```bash
//! evorule-server --addr 0.0.0.0:18080 --auth-token secret123
//! ```
//!
//! 所有参数均支持环境变量覆盖（前缀 `EVORULE_`）。

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;
use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;
use tier2_governance::api::auth::AuthConfig;
use tier2_governance::api::server::{AppState, GovernanceApi, GovernanceServer, SessionApi};
use tier2_governance::auditor::Auditor;
use tier2_governance::io_dispatcher::IoDispatcher;
use tier2_governance::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
    memory_handler::MemoryHandler, tool_handler::ToolHandler,
};
use tier2_governance::io_subscriber::IoSubscriber;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// evorule-server 启动配置
///
/// 所有字段均可通过 CLI 参数或环境变量（前缀 `EVORULE_`）提供。
#[derive(Parser, Debug)]
#[command(
    name = "evorule-server",
    version,
    about = "TheEquation 治理层 HTTP 服务"
)]
struct Cli {
    /// 监听地址
    #[arg(long, default_value = "0.0.0.0:18080", env = "EVORULE_ADDR")]
    addr: String,

    /// Bearer 认证 token（未提供时禁用认证，仅用于开发）
    #[arg(long, env = "EVORULE_AUTH_TOKEN")]
    auth_token: Option<String>,

    /// 宪法文件路径（core_eval.json，不可热重载）
    #[arg(
        long,
        default_value = "./tier0-tcb/core_eval.json",
        env = "EVORULE_CORE_EVAL"
    )]
    core_eval: PathBuf,

    /// 业务规则目录（热重载监听目录）
    #[arg(long, default_value = "./rules", env = "EVORULE_RULES_DIR")]
    rules_dir: PathBuf,

    /// SQLite 数据库文件路径
    #[arg(long, default_value = "./data/evorule.db", env = "EVORULE_DB_PATH")]
    db_path: PathBuf,

    /// Memory handler 存储根目录
    #[arg(long, default_value = "./data/memory", env = "EVORULE_MEMORY_DIR")]
    memory_dir: PathBuf,

    /// LLM API Key（OpenAI 兼容）
    #[arg(long, env = "EVORULE_LLM_API_KEY")]
    llm_api_key: Option<String>,

    /// LLM API 基础 URL（默认 OpenAI 官方）
    #[arg(long, env = "EVORULE_LLM_BASE_URL")]
    llm_base_url: Option<String>,

    /// 默认 LLM 模型标识
    #[arg(long, default_value = "gpt-4o-mini", env = "EVORULE_LLM_MODEL")]
    llm_model: String,

    /// 反应器最大指令执行步数
    #[arg(long, default_value_t = 1000, env = "EVORULE_MAX_ROUNDS")]
    max_rounds: usize,

    /// 日志级别（error/warn/info/debug/trace）
    #[arg(long, default_value = "info", env = "EVORULE_LOG_LEVEL")]
    log_level: String,
}

/// 将 `serde_json::Value` 转换为 `tier0_tcb::JsonValue`
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

/// 加载 core_eval.json 并转换为 transform 列表
fn load_core_eval(path: &PathBuf) -> Result<Vec<JsonValue>, String> {
    let json_str = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 core_eval.json 失败 {}: {}", path.display(), e))?;
    let json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("解析 core_eval.json 失败: {}", e))?;
    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default();
    Ok(transform)
}

/// 确保目录存在
fn ensure_dir(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| format!("创建目录失败 {}: {}", path.display(), e))?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. 初始化日志
    let filter = EnvFilter::try_new(&cli.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("=== evorule-server 启动中 ===");
    info!("监听地址: {}", cli.addr);
    info!("宪法路径: {}", cli.core_eval.display());
    info!("规则目录: {}", cli.rules_dir.display());
    info!("数据库: {}", cli.db_path.display());
    info!("Memory 目录: {}", cli.memory_dir.display());
    info!(
        "LLM 模型: {} (base_url: {:?})",
        cli.llm_model, cli.llm_base_url
    );
    info!(
        "认证: {}",
        if cli.auth_token.is_some() {
            "已启用"
        } else {
            "已禁用（开发模式）"
        }
    );

    // 2. 加载 core_eval.json（宪法）
    let core_eval = load_core_eval(&cli.core_eval)?;
    info!("已加载 {} 条 transform 规则", core_eval.len());

    // 3. 确保数据目录存在
    if let Some(parent) = cli.db_path.parent() {
        ensure_dir(&parent.to_path_buf())?;
    }
    ensure_dir(&cli.memory_dir)?;

    // 4. 初始化 5 个 I/O handler
    let api_key = cli
        .llm_api_key
        .clone()
        .unwrap_or_else(|| "dummy_key".to_string());
    let llm = LlmHandler::with_model(api_key, cli.llm_base_url.clone(), cli.llm_model.clone());
    let db = DbHandler::connect_file(&cli.db_path)
        .await
        .map_err(|e| format!("数据库连接失败: {}", e))?;
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(cli.memory_dir.clone());
    let tool = ToolHandler::new();
    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    info!("[1/4] I/O handler 已初始化（LLM/DB/HTTP/Memory/Tool）");

    // 5. 创建单反应器（GovernanceApi 向后兼容路由用）
    let reactor = Reactor::builder(core_eval.clone())
        .max_rounds(cli.max_rounds)
        .build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    // 6. spawn IoSubscriber 任务（订阅 event，执行 I/O，回写 IoResponse）
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    info!("[2/4] 反应器 + I/O 订阅者已启动");

    // 7. 创建审计器 + GovernanceApi + SessionApi + AppState
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new(core_eval, cli.max_rounds);
    let state = AppState::new(api, session_api);

    info!("[3/4] 审计器 + GovernanceApi + SessionApi 已创建");

    // 8. 构建服务器（带认证）
    let auth = match &cli.auth_token {
        Some(token) => AuthConfig::new(vec![token.clone()], true),
        None => AuthConfig::disabled(),
    };
    let server = GovernanceServer::new(state, auth, cli.addr.clone());

    info!("[4/4] HTTP 服务器已就绪，监听 {}", cli.addr);
    info!("=== evorule-server 启动完成 ===");
    info!("端点：");
    info!("  健康检查: GET  http://{}/api/health", cli.addr);
    info!("  创建会话: POST http://{}/api/sessions", cli.addr);
    info!(
        "  提交命令: POST http://{}/api/sessions/{{id}}/command",
        cli.addr
    );
    info!(
        "  查询状态: GET  http://{}/api/sessions/{{id}}/state",
        cli.addr
    );
    info!(
        "  SSE 事件: GET  http://{}/api/sessions/{{id}}/events",
        cli.addr
    );
    info!("  审计报告: GET  http://{}/api/audit", cli.addr);
    info!("按 Ctrl+C 优雅退出");

    // 9. 启动服务器（带优雅退出）
    let listener = tokio::net::TcpListener::bind(&cli.addr).await?;
    let router = server.build_router();
    let serve = axum::serve(listener, router);

    // 优雅退出：等待 Ctrl+C
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        info!("收到 Ctrl+C 信号，开始优雅退出...");
    };

    if let Err(e) = serve.with_graceful_shutdown(shutdown).await {
        tracing::error!("服务器退出错误: {}", e);
        return Err(e.into());
    }

    info!("evorule-server 已停止");
    Ok(())
}
