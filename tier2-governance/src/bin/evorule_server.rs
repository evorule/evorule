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
use tracing_appender::rolling::{RollingFileAppender, Rotation};

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
///     "format": "json",
///     "file": "./logs/evorule.log"
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
    /// WAL 文件存储目录（可选，指定后启用 WAL 持久化）
    wal_dir: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileLogConfig {
    level: Option<String>,
    /// `plain` 或 `json`（P2-7）
    format: Option<String>,
    /// 日志文件路径（生产环境持久化，可选）
    file: Option<PathBuf>,
    /// 日志文件保留天数（默认 7 天）
    max_days: Option<u32>,
    /// 日志目录最大占用空间（MB，默认 1024MB）
    max_size_mb: Option<u64>,
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

    /// 日志文件路径（生产环境持久化，可选）
    #[arg(long, env = "EVORULE_LOG_FILE")]
    log_file: Option<PathBuf>,

    /// 日志文件保留天数（默认 7 天）
    #[arg(long, env = "EVORULE_LOG_MAX_DAYS")]
    log_max_days: Option<u32>,

    /// 日志目录最大占用空间（MB，默认 1024MB）
    #[arg(long, env = "EVORULE_LOG_MAX_SIZE_MB")]
    log_max_size_mb: Option<u64>,

    /// WAL 文件存储目录（可选，指定后启用 WAL 持久化）
    #[arg(long, env = "EVORULE_WAL_DIR")]
    wal_dir: Option<PathBuf>,

    /// WAL fsync 开关（P02：启用后在每次 WAL 写入后执行 fsync，确保断电时数据不丢失）
    #[arg(long, env = "EVORULE_WAL_FSYNC")]
    wal_fsync: bool,

    /// WAL 文件最大大小（MB，P03：达到此大小后自动轮换文件，默认 100MB，0 表示不轮换）
    #[arg(long, env = "EVORULE_WAL_MAX_SIZE_MB")]
    wal_max_size_mb: Option<u64>,

    /// 启用审计链实时验证（P06：每次 audit_new 后自动验证审计链完整性）
    #[arg(long, env = "EVORULE_AUTO_VERIFY")]
    auto_verify: bool,

    /// 自动验证阈值（P06：审计条目数超过此值时跳过验证，0 表示不限制，默认 1000）
    #[arg(long, env = "EVORULE_AUTO_VERIFY_THRESHOLD")]
    auto_verify_threshold: Option<usize>,

    /// 自动验证间隔（P06：每 N 次 audit_new 验证一次，默认 1）
    #[arg(long, env = "EVORULE_AUTO_VERIFY_INTERVAL")]
    auto_verify_interval: Option<usize>,
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
    log_file: Option<PathBuf>,
    log_max_days: u32,
    log_max_size_mb: u64,
    /// WAL 文件存储目录（可选，指定后启用 WAL 持久化）
    wal_dir: Option<PathBuf>,
    /// WAL fsync 开关（P02：启用后在每次 WAL 写入后执行 fsync）
    wal_fsync: bool,
    /// WAL 文件最大大小（字节，P03：达到此大小后自动轮换文件）
    max_wal_size_bytes: u64,
    /// 是否启用审计链实时验证（P06）
    auto_verify: bool,
    /// 自动验证阈值（P06，0 表示不限制）
    auto_verify_threshold: usize,
    /// 自动验证间隔（P06，1 表示每次都验证）
    auto_verify_interval: usize,
}

impl ResolvedConfig {
    /// 按 CLI > env > file > default 优先级解析配置
    fn resolve(cli: Cli, file: FileConfig) -> Self {
        let max_wal_size_mb = cli.wal_max_size_mb.unwrap_or(100);
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
            log_file: cli.log_file.or(file.log.file),
            log_max_days: cli.log_max_days.or(file.log.max_days).unwrap_or(7),
            log_max_size_mb: cli.log_max_size_mb.or(file.log.max_size_mb).unwrap_or(1024),
            wal_dir: cli.wal_dir.or(file.paths.wal_dir),
            wal_fsync: cli.wal_fsync,
            max_wal_size_bytes: max_wal_size_mb * 1024 * 1024,
            auto_verify: cli.auto_verify,
            auto_verify_threshold: cli.auto_verify_threshold.unwrap_or(1000),
            auto_verify_interval: cli.auto_verify_interval.unwrap_or(1),
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

/// 初始化日志订阅器（P2-7：支持 plain 和 json 两种格式，支持文件持久化）
///
/// # 参数
/// - `level`: 日志级别（error/warn/info/debug/trace）
/// - `format`: 日志格式（plain/json）
/// - `log_file`: 日志文件路径（可选，指定后启用文件持久化）
fn init_logging(level: &str, format: &str, log_file: Option<&PathBuf>) {
    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let is_json = format.to_lowercase() == "json";

    if let Some(file_path) = log_file {
        let dir = file_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let file_name = file_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("evorule.log"));

        let appender = RollingFileAppender::new(Rotation::DAILY, dir, file_name);
        let (non_blocking, _guard) = tracing_appender::non_blocking(appender);

        let builder = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(non_blocking);

        if is_json {
            let _ = tracing::subscriber::set_global_default(builder.json().finish());
        } else {
            let _ = tracing::subscriber::set_global_default(builder.finish());
        }
        return;
    }

    let builder = tracing_subscriber::fmt().with_env_filter(filter);

    if is_json {
        let _ = tracing::subscriber::set_global_default(builder.json().finish());
    } else {
        let _ = tracing::subscriber::set_global_default(builder.finish());
    }
}

async fn log_cleanup_task(log_dir: PathBuf, max_days: u32, max_size_mb: u64) {
    let interval = Duration::from_secs(3600);
    let max_size_bytes = max_size_mb * 1024 * 1024;

    loop {
        tokio::time::sleep(interval).await;

        if !log_dir.exists() {
            continue;
        }

        let mut log_files = match std::fs::read_dir(&log_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let fname = e.file_name();
                    let name = fname.to_string_lossy();
                    name.starts_with("evorule") && name.ends_with(".log")
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                warn!("读取日志目录失败 {}: {}", log_dir.display(), e);
                continue;
            }
        };

        log_files.sort_by(|a, b| {
            a.metadata()
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH)
                .cmp(
                    &b.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH),
                )
        });

        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs()
            - (max_days as u64) * 24 * 3600;

        let mut deleted_count = 0;
        for entry in &log_files {
            let mtime = match entry.metadata().and_then(|m| m.modified()) {
                Ok(t) => t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Err(_) => continue,
            };

            if mtime < cutoff {
                if let Err(e) = std::fs::remove_file(entry.path()) {
                    warn!("删除过期日志文件失败 {}: {}", entry.path().display(), e);
                } else {
                    deleted_count += 1;
                }
            }
        }

        if deleted_count > 0 {
            info!("已清理 {} 个过期日志文件", deleted_count);
        }

        let total_size: u64 = log_files
            .iter()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        if total_size > max_size_bytes {
            let mut to_delete = Vec::new();
            let mut current_size = total_size;

            for entry in &log_files {
                if current_size <= max_size_bytes {
                    break;
                }

                let fname = entry.file_name();
                let name = fname.to_string_lossy();
                if name == "evorule.log" {
                    continue;
                }

                if let Ok(len) = entry.metadata().map(|m| m.len()) {
                    to_delete.push(entry.path().clone());
                    current_size -= len;
                }
            }

            for path in &to_delete {
                if let Err(e) = std::fs::remove_file(path) {
                    warn!("删除日志文件失败 {}: {}", path.display(), e);
                }
            }

            if !to_delete.is_empty() {
                info!(
                    "已清理 {} 个日志文件以释放空间，原大小: {}MB，目标: {}MB",
                    to_delete.len(),
                    total_size / 1024 / 1024,
                    max_size_mb
                );
            }
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

    // 1. 初始化日志（P2-7: 支持 JSON 结构化日志，支持文件持久化）
    init_logging(&cfg.log_level, &cfg.log_format, cfg.log_file.as_ref());

    let start_time = Instant::now();
    info!("=== evorule-server 启动中 ===");
    info!("监听地址: {}", cfg.addr);
    info!("宪法路径: {}", cfg.core_eval.display());
    info!("规则目录: {}", cfg.rules_dir.display());
    info!("数据库: {}", cfg.db_path.display());
    info!("Memory 目录: {}", cfg.memory_dir.display());
    info!("日志格式: {}", cfg.log_format);
    info!(
        "日志输出: {}",
        if let Some(file) = &cfg.log_file {
            format!("文件: {}", file.display())
        } else {
            "控制台".to_string()
        }
    );
    info!(
        "日志清理: 保留 {} 天, 最大 {}MB",
        cfg.log_max_days, cfg.log_max_size_mb
    );
    info!(
        "认证: {}",
        if cfg.auth_token.is_some() {
            "已启用"
        } else {
            "已禁用（开发模式）"
        }
    );
    info!(
        "WAL: {}",
        if let Some(dir) = &cfg.wal_dir {
            format!(
                "目录: {}, fsync: {}, 最大大小: {}MB",
                dir.display(),
                cfg.wal_fsync,
                cfg.max_wal_size_bytes / (1024 * 1024)
            )
        } else {
            "已禁用（纯内存模式）".to_string()
        }
    );
    info!(
        "实时审计验证: {}",
        if cfg.auto_verify {
            format!(
                "已启用（阈值: {}, 间隔: {}）",
                cfg.auto_verify_threshold, cfg.auto_verify_interval
            )
        } else {
            "已禁用".to_string()
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
    if let Some(wal_dir) = &cfg.wal_dir {
        ensure_dir(wal_dir)?;
    }
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

    // 7. spawn 日志清理任务（定期清理过期和超大日志文件）
    if let Some(log_file) = &cfg.log_file {
        if let Some(log_dir) = log_file.parent() {
            let log_dir = log_dir.to_path_buf();
            let max_days = cfg.log_max_days;
            let max_size_mb = cfg.log_max_size_mb;
            tokio::spawn(async move {
                log_cleanup_task(log_dir, max_days, max_size_mb).await;
            });
        }
    }

    info!(
        "[2/4] 反应器 + I/O 订阅者已启动（耗时: {}ms）",
        step_start.elapsed().as_millis()
    );

    // 8. 创建审计器 + GovernanceApi + SessionApi + AppState
    let step_start = Instant::now();
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new_with_full_config(
        core_eval,
        cfg.max_rounds,
        cfg.wal_dir.clone(),
        cfg.wal_fsync,
        cfg.max_wal_size_bytes,
        cfg.auto_verify,
        cfg.auto_verify_threshold,
        cfg.auto_verify_interval,
    );
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
        None => {
            // 警告：如果绑定到非 loopback 且未设 auth token，提示用户加 --auth-token
            let is_non_loopback = !cfg.addr.starts_with("127.")
                && !cfg.addr.starts_with("localhost")
                && cfg.addr != "0.0.0.0:0";
            if is_non_loopback {
                warn!(
                    "🔓 认证已禁用,但服务器绑定到 {} (非 loopback)。\n\
                     生产环境必须设置 --auth-token 或 EVORULE_AUTH_TOKEN 环境变量!\n\
                     否则任何能访问该地址的进程都能读/写所有 session 数据。",
                    cfg.addr
                );
            } else {
                info!("🔓 认证已禁用 (loopback 模式,仅适合开发)");
            }
            AuthConfig::disabled()
        }
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
