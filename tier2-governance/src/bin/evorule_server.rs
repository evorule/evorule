// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-server —— 独立二进制服务入口
//!
//! 启动 GovernanceServer（HTTP API + SSE 事件流 + 多会话管理），
//! 内置 IoSubscriber（DB / HTTP / Memory 三种 I/O handler）。
//!
//! # 用法
//! ```bash
//! evorule-server --addr 0.0.0.0:18080 --auth-token secret123
//! evorule-server --config evorule.toml --log-format json
//! ```
//!
//! # 配置加载优先级（P2-9）
//! CLI 参数 > 环境变量（前缀 `EVORULE_`）> TOML 配置文件 > 内置默认值
//!
//! # 优雅退出（P2-8）
//! - 监听 SIGTERM（Docker 停止信号）和 SIGINT（Ctrl+C）
//! - 收到信号后：readiness 设为 false（负载均衡器切走流量）→ 等待进行中请求 → 30s 超时强制退出
//! - `GET /api/health/liveness` 始终 200；`GET /api/health/readiness` 在退出期间返回 503

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use std::time::Instant;
use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;
use tier2_governance::api::auth::AuthConfig;
use tier2_governance::api::server::{AppState, GovernanceApi, GovernanceServer, SessionApi};
use tier2_governance::auditor::Auditor;
use tier2_governance::io_dispatcher::IoDispatcher;
use tier2_governance::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, memory_handler::MemoryHandler,
};
use tier2_governance::io_subscriber::IoSubscriber;
use tier2_governance::metrics::{Metrics, SharedMetrics};
use tier2_governance::shared_facts_log::SharedFactsLog;
use tracing::{error, info, warn};

/// 优雅退出超时（P2-8：等待进行中请求的最长时间）
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// 优雅退出期间状态检查间隔
const SHUTDOWN_CHECK_INTERVAL: Duration = Duration::from_secs(5);

// ===== P2-9：JSON 配置文件结构 =====

/// JSON 配置文件顶层结构
///
/// 示例文件 (`evorule.json`)：
/// ```json
/// {
///   "server": {
///     "addr": "0.0.0.0:18080",
///     "max_rounds": 1000
///   },
///   "auth": {
///     "token": "secret123"
///   },
///   "paths": {
///     "core_eval": "./tier0-tcb/core_eval.json",
///     "rules_dir": "./rules",
///     "db_path": "./data/evorule.db",
///     "memory_dir": "./data/memory"
///   },
///   "log": {
///     "level": "info",
///     "format": "json"
///   }
/// }
/// ```
///
/// **为什么用 JSON?**
/// EvoRule 的核心理念是"只接受和运行 JSON 数据集"。
/// 配置文件虽然不是业务规则,但也应该是 JSON,以保持原则一致性。
/// v6.0 之前使用 TOML,v6.1 起迁移到 JSON。
#[derive(Debug, Default, serde::Deserialize)]
struct FileConfig {
    #[serde(default)]
    server: FileServerConfig,
    #[serde(default)]
    auth: FileAuthConfig,
    #[serde(default)]
    paths: FilePathsConfig,
    #[serde(default)]
    log: FileLogConfig,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileServerConfig {
    addr: Option<String>,
    max_rounds: Option<usize>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileAuthConfig {
    token: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FilePathsConfig {
    core_eval: Option<PathBuf>,
    rules_dir: Option<PathBuf>,
    db_path: Option<PathBuf>,
    memory_dir: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileLogConfig {
    level: Option<String>,
    /// `plain` 或 `json`（P2-7）
    format: Option<String>,
}

/// 加载 JSON 配置文件
///
/// 文件不存在时返回空配置（不报错，允许纯 CLI 启动）。
fn load_config_file(path: &Option<PathBuf>) -> FileConfig {
    match path {
        Some(p) if p.exists() => {
            let content = match std::fs::read_to_string(p) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        "读取配置文件失败 {}: {}，将仅使用 CLI/环境变量",
                        p.display(),
                        e
                    );
                    return FileConfig::default();
                }
            };
            match serde_json::from_str::<FileConfig>(&content) {
                Ok(cfg) => {
                    info!("已加载配置文件: {}", p.display());
                    cfg
                }
                Err(e) => {
                    warn!(
                        "解析配置文件失败 {}: {}，将仅使用 CLI/环境变量",
                        p.display(),
                        e
                    );
                    FileConfig::default()
                }
            }
        }
        Some(p) => {
            warn!("配置文件不存在: {}，将仅使用 CLI/环境变量", p.display());
            FileConfig::default()
        }
        None => FileConfig::default(),
    }
}

/// evorule-server 启动配置
///
/// 所有字段均可通过 CLI 参数、环境变量（前缀 `EVORULE_`）或 JSON 配置文件提供。
/// 优先级：CLI > 环境变量 > 配置文件 > 内置默认值。
#[derive(Parser, Debug)]
#[command(
    name = "evorule-server",
    version,
    about = "TheEquation 治理层 HTTP 服务"
)]
struct Cli {
    /// JSON 配置文件路径（P2-9，可选，例: ./evorule.json）
    #[arg(long, env = "EVORULE_CONFIG")]
    config: Option<PathBuf>,

    /// 监听地址
    #[arg(long, env = "EVORULE_ADDR")]
    addr: Option<String>,

    /// Bearer 认证 token（未提供时禁用认证，仅用于开发）
    #[arg(long, env = "EVORULE_AUTH_TOKEN")]
    auth_token: Option<String>,

    /// 宪法文件路径（core_eval.json，不可热重载）
    #[arg(long, env = "EVORULE_CORE_EVAL")]
    core_eval: Option<PathBuf>,

    /// 业务规则目录（热重载监听目录）
    #[arg(long, env = "EVORULE_RULES_DIR")]
    rules_dir: Option<PathBuf>,

    /// SQLite 数据库文件路径
    #[arg(long, env = "EVORULE_DB_PATH")]
    db_path: Option<PathBuf>,

    /// Memory handler 存储根目录
    #[arg(long, env = "EVORULE_MEMORY_DIR")]
    memory_dir: Option<PathBuf>,

    /// 反应器最大指令执行步数
    #[arg(long, env = "EVORULE_MAX_ROUNDS")]
    max_rounds: Option<usize>,

    /// 日志级别（error/warn/info/debug/trace）
    #[arg(long, env = "EVORULE_LOG_LEVEL")]
    log_level: Option<String>,

    /// 日志格式（P2-7：`plain` 或 `json`，默认 `plain`）
    #[arg(long, env = "EVORULE_LOG_FORMAT")]
    log_format: Option<String>,
}

/// 合并后的最终配置（CLI > env > file > default）
struct ResolvedConfig {
    addr: String,
    auth_token: Option<String>,
    core_eval: PathBuf,
    rules_dir: PathBuf,
    db_path: PathBuf,
    memory_dir: PathBuf,
    max_rounds: usize,
    log_level: String,
    log_format: String,
}

impl ResolvedConfig {
    /// 按 CLI > env > file > default 优先级解析配置
    fn resolve(cli: Cli, file: FileConfig) -> Self {
        Self {
            addr: cli
                .addr
                .or(file.server.addr)
                .unwrap_or_else(|| "0.0.0.0:18080".to_string()),
            auth_token: cli.auth_token.or(file.auth.token),
            core_eval: cli
                .core_eval
                .or(file.paths.core_eval)
                .unwrap_or_else(|| PathBuf::from("./tier0-tcb/core_eval.json")),
            rules_dir: cli
                .rules_dir
                .or(file.paths.rules_dir)
                .unwrap_or_else(|| PathBuf::from("./rules")),
            db_path: cli
                .db_path
                .or(file.paths.db_path)
                .unwrap_or_else(|| PathBuf::from("./data/evorule.db")),
            memory_dir: cli
                .memory_dir
                .or(file.paths.memory_dir)
                .unwrap_or_else(|| PathBuf::from("./data/memory")),
            max_rounds: cli.max_rounds.or(file.server.max_rounds).unwrap_or(1000),
            log_level: cli
                .log_level
                .or(file.log.level)
                .unwrap_or_else(|| "info".to_string()),
            log_format: cli
                .log_format
                .or(file.log.format)
                .unwrap_or_else(|| "plain".to_string()),
        }
    }
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

/// 初始化日志订阅器（P2-7：支持 plain 和 json 两种格式）
fn init_logging(level: &str, format: &str) {
    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match format.to_lowercase().as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        _ => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // P2-9: 加载 TOML 配置文件（若指定）
    let file_config = load_config_file(&cli.config);

    // 按 CLI > env > file > default 优先级解析
    let cfg = ResolvedConfig::resolve(cli, file_config);

    // 1. 初始化日志（P2-7: 支持 JSON 结构化日志）
    init_logging(&cfg.log_level, &cfg.log_format);

    let start_time = Instant::now();
    info!("=== evorule-server 启动中 ===");
    info!("监听地址: {}", cfg.addr);
    info!("宪法路径: {}", cfg.core_eval.display());
    info!("规则目录: {}", cfg.rules_dir.display());
    info!("数据库: {}", cfg.db_path.display());
    info!("Memory 目录: {}", cfg.memory_dir.display());
    info!("日志格式: {}", cfg.log_format);
    info!(
        "认证: {}",
        if cfg.auth_token.is_some() {
            "已启用"
        } else {
            "已禁用（开发模式）"
        }
    );

    // 2. 加载 core_eval.json（宪法）
    let step_start = Instant::now();
    let core_eval = load_core_eval(&cfg.core_eval)?;
    info!(
        "已加载 {} 条 transform 规则（耗时: {}ms）",
        core_eval.len(),
        step_start.elapsed().as_millis()
    );

    // 3. 确保数据目录存在
    let step_start = Instant::now();
    if let Some(parent) = cfg.db_path.parent() {
        ensure_dir(&parent.to_path_buf())?;
    }
    ensure_dir(&cfg.memory_dir)?;
    info!(
        "数据目录检查完成（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    // 4. 初始化 3 个 I/O handler
    let step_start = Instant::now();
    let db = DbHandler::connect_file(&cfg.db_path)
        .await
        .map_err(|e| format!("数据库连接失败: {}", e))?;
    info!(
        "数据库连接完成（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    let http = HttpHandler::new();
    let memory = MemoryHandler::new(cfg.memory_dir.clone());
    let dispatcher = IoDispatcher::new(db, http, memory);

    // P2-7: 创建 Prometheus 指标（共享给 IoSubscriber + AppState）
    let metrics: SharedMetrics =
        Arc::new(Metrics::new().map_err(|e| format!("指标初始化失败: {}", e))?);

    // P2-7: IoSubscriber 注入 metrics，记录 I/O 耗时和错误
    let subscriber = IoSubscriber::new(dispatcher).with_metrics(metrics.clone());

    info!(
        "[1/4] I/O handler 已初始化（DB/HTTP/Memory）（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    // 5. 创建单反应器（GovernanceApi 向后兼容路由用）
    let reactor = Reactor::builder(core_eval.clone())
        .max_rounds(cfg.max_rounds)
        .build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    // 6. spawn IoSubscriber 任务（订阅 event，执行 I/O，回写 IoResponse）
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    info!(
        "[2/4] 反应器 + I/O 订阅者已启动（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    // 7. 创建审计器 + GovernanceApi + SessionApi + AppState
    let step_start = Instant::now();
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new(core_eval, cfg.max_rounds);
    session_api.start_reaper();

    // P2-8: 创建 readiness flag（优雅退出时设为 false）
    let readiness: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));

    // P1-1: 创建跨会话共享事实存储
    let shared_facts = SharedFactsLog::new();

    // P2-7/P2-8: AppState 注入 metrics 和 readiness
    let state = AppState::new(
        api,
        session_api,
        metrics.clone(),
        readiness.clone(),
        shared_facts,
    );

    info!(
        "[3/4] 审计器 + GovernanceApi + SessionApi 已创建（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    // 8. 构建服务器（带认证）
    let step_start = Instant::now();
    let auth = match &cfg.auth_token {
        Some(token) => AuthConfig::new(vec![token.clone()], true),
        None => AuthConfig::disabled(),
    };
    let server = GovernanceServer::new(state, auth, cfg.addr.clone());

    info!(
        "[4/4] HTTP 服务器已就绪，监听 {}（耗时: {}ms）",
        cfg.addr,
        step_start.elapsed().as_millis()
    );
    info!(
        "=== evorule-server 启动完成（总耗时: {}ms）===",
        start_time.elapsed().as_millis()
    );
    info!("端点：");
    info!("  健康检查: GET  http://{}/api/health", cfg.addr);
    info!("  Liveness: GET  http://{}/api/health/liveness", cfg.addr);
    info!("  Readiness: GET http://{}/api/health/readiness", cfg.addr);
    info!("  Metrics:  GET  http://{}/metrics", cfg.addr);
    info!("  创建会话: POST http://{}/api/sessions", cfg.addr);
    info!(
        "  提交命令: POST http://{}/api/sessions/{{id}}/command",
        cfg.addr
    );
    info!(
        "  查询状态: GET  http://{}/api/sessions/{{id}}/state",
        cfg.addr
    );
    info!(
        "  SSE 事件: GET  http://{}/api/sessions/{{id}}/events",
        cfg.addr
    );
    info!("  审计报告: GET  http://{}/api/audit", cfg.addr);
    info!("  Agent 类型: GET  http://{}/api/agents/types", cfg.addr);
    info!("  启动 Agent: POST http://{}/api/agents/run", cfg.addr);
    info!(
        "  Agent 状态: GET  http://{}/api/agents/{{id}}/status",
        cfg.addr
    );
    info!(
        "  Agent 结果: GET  http://{}/api/agents/{{id}}/result",
        cfg.addr
    );
    info!(
        "优雅退出：SIGTERM/SIGINT → readiness=false → 等待 {}s",
        GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
    );

    // 9. 启动服务器（带优雅退出，P2-8）
    // P1-4: 使用 into_make_service_with_connect_info 注入客户端 IP，
    // 以支持 GovernorLayer（速率限制）按 IP 限流
    let listener = tokio::net::TcpListener::bind(&cfg.addr).await?;
    let router = server.build_router();
    let serve = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    );

    // P2-8: 优雅退出信号处理
    let readiness_flag = readiness.clone();
    let shutdown = async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .unwrap_or_else(|e| panic!("install SIGTERM handler: {}", e));
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("收到 SIGINT (Ctrl+C) 信号，开始优雅退出...");
                }
                _ = sigterm.recv() => {
                    info!("收到 SIGTERM 信号，开始优雅退出...");
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            info!("收到 Ctrl+C 信号，开始优雅退出...");
        }

        // P2-8: 标记不就绪，负载均衡器切走流量
        readiness_flag.store(false, Ordering::SeqCst);
        info!("已标记为不就绪（readiness=false），/api/health/readiness 将返回 503");

        // 定期输出优雅退出状态，便于排查卡住的步骤
        let total_timeout = GRACEFUL_SHUTDOWN_TIMEOUT.as_secs();
        let check_interval = SHUTDOWN_CHECK_INTERVAL.as_secs();
        let mut elapsed = 0u64;

        while elapsed < total_timeout {
            tokio::time::sleep(SHUTDOWN_CHECK_INTERVAL).await;
            elapsed += check_interval;
            let remaining = total_timeout - elapsed;

            warn!("优雅退出中，已等待 {}s，剩余 {}s...", elapsed, remaining);
        }

        error!(
            "优雅退出超时前最后状态：已等待 {}s，即将强制结束",
            total_timeout
        );
    };

    // P2-8: 优雅退出 + 30s 超时
    let graceful = serve.with_graceful_shutdown(shutdown);
    match graceful.await {
        Ok(()) => {
            info!("服务器已优雅退出");
        }
        Err(e) => {
            error!("服务器退出错误: {}", e);
            return Err(e.into());
        }
    }

    info!("evorule-server 已停止");
    Ok(())
}
