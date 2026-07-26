// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 业务规则热重载
//!
//! 监听 `core_eval.json` 文件变化，自动重新加载 transform 列表。
//! 通过 `tokio::sync::watch` 通道通知上层组件（如反应器）使用新配置。
//!
//! # 设计
//! - 使用 `notify` crate 监听文件系统事件
//! - 文件变化时重新读取并解析 JSON
//! - 通过 `watch::Sender<Vec<JsonValue>>` 广播新配置
//! - 上层通过 `watch::Receiver` 获取最新配置

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};
use tier1_reactor::{ControlFlowType, IoType};
use tokio::sync::watch;
use tracing::{error, info, warn};

/// 热重载错误
#[derive(Debug, thiserror::Error)]
pub enum HotReloadError {
    /// 文件读取失败
    #[error("Failed to read config file: {0}")]
    ReadError(String),

    /// JSON 解析失败
    #[error("Failed to parse JSON: {0}")]
    ParseError(String),

    /// 文件监听失败
    #[error("Failed to watch file: {0}")]
    WatchError(String),

    /// 配置验证失败
    #[error("Config validation failed: {0}")]
    ValidationError(String),
}

/// 配置验证结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// 是否通过验证
    pub valid: bool,
    /// 错误信息列表
    pub errors: Vec<String>,
    /// 警告信息列表
    pub warnings: Vec<String>,
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
            let mut map = std::collections::BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

/// 加载 core_eval.json 并转换为 transform 列表
pub fn load_core_eval(path: &PathBuf) -> Result<Vec<JsonValue>, HotReloadError> {
    let json_str = std::fs::read_to_string(path)
        .map_err(|e| HotReloadError::ReadError(format!("{}: {}", path.display(), e)))?;

    let json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| HotReloadError::ParseError(e.to_string()))?;

    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default();

    Ok(transform)
}

/// 验证 core_eval 配置合法性（干跑测试）
///
/// 通过在沙箱 payload 上预执行所有支持的指令类型，验证 core_eval 的 transform 列表是否有效。
/// 这确保热重载的配置不会导致反应器运行时崩溃。
// 沙箱预执行所有指令类型, 拆函数需共享 core_eval/registry 状态。详见 GATE_REFERENCE.md §六(豁免索引)
#[allow(clippy::too_many_lines)]
pub fn validate_core_eval(core_eval: &[JsonValue]) -> ValidationResult {
    let mut result = ValidationResult {
        valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    // 沙箱 payload（空对象）
    let sandbox_payload = JsonValue::Object(std::collections::BTreeMap::new());

    // 测试指令列表：覆盖 core_eval.json 中所有合法指令类型
    let test_instructions = vec![
        // 原子计算指令
        make_test_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ],
        ),
        make_test_instruction(
            "decrement",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ],
        ),
        make_test_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(42)),
            ],
        ),
        // 控制流指令
        make_test_instruction(
            ControlFlowType::Sequence.as_str(),
            &[(
                "instructions",
                JsonValue::array(vec![make_test_instruction("noop", &[])]),
            )],
        ),
        make_test_instruction(
            ControlFlowType::Conditional.as_str(),
            &[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                        ("inner", JsonValue::empty_array()),
                    ]),
                ),
                ("then", make_test_instruction("noop", &[])),
                ("else", make_test_instruction("noop", &[])),
            ],
        ),
        make_test_instruction(
            ControlFlowType::WhileLoop.as_str(),
            &[
                (
                    "condition",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                        ("inner", JsonValue::empty_array()),
                    ]),
                ),
                ("body", make_test_instruction("noop", &[])),
            ],
        ),
        // I/O 指令
        make_test_instruction(
            IoType::CALL_EXTERNAL.as_str(),
            &[("url", JsonValue::string("https://example.com"))],
        ),
        make_test_instruction(
            IoType::QUERY_DB.as_str(),
            &[
                ("query", JsonValue::string("SELECT 1")),
                ("params", JsonValue::empty_array()),
            ],
        ),
        make_test_instruction(
            IoType::HTTP_GET.as_str(),
            &[
                ("url", JsonValue::string("http://localhost")),
                ("headers", JsonValue::empty_object()),
                ("timeout_ms", JsonValue::Integer(1000)),
            ],
        ),
        make_test_instruction(
            IoType::SAVE_MEMORY.as_str(),
            &[
                ("key", JsonValue::string("test")),
                ("value", JsonValue::string("data")),
            ],
        ),
        make_test_instruction(
            IoType::CALL_SERVICE.as_str(),
            &[
                ("service_name", JsonValue::string("test")),
                ("args", JsonValue::empty_object()),
            ],
        ),
        // 兜底指令
        make_test_instruction("noop", &[]),
    ];

    // 干跑测试：在沙箱上执行每条指令
    for instr in &test_instructions {
        let instr_type = instr
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        match execute_transition(core_eval, instr, &sandbox_payload, &[]) {
            Ok(TransitionResult::State { .. }) => {
                // 正常转换
            }
            Ok(TransitionResult::IoRequired { .. }) => {
                // I/O 请求是正常行为（I/O 指令预期会触发 IoRequired）
            }
            Err(e) => {
                result.valid = false;
                result
                    .errors
                    .push(format!("instruction type '{}' failed: {}", instr_type, e));
            }
        }
    }

    // 检查是否有 all([]) 兜底规则
    let has_catch_all = core_eval.iter().any(|rule| {
        if let Some(rule_type) = rule.get("type").and_then(|v| v.as_str()) {
            if rule_type == "branch" {
                if let Some(params) = rule.get("params") {
                    if let Some(domain) = params.get("domain") {
                        if let Some(domain_type) = domain.get("type").and_then(|v| v.as_str()) {
                            if domain_type == "all" {
                                if let Some(inner) = domain.get("inner").and_then(|v| v.as_array())
                                {
                                    return inner.is_empty();
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    });

    if !has_catch_all {
        result
            .warnings
            .push("Missing all([]) catch-all rule".to_string());
    }

    // 检查 transform 列表长度
    if core_eval.is_empty() {
        result.warnings.push("Empty transform list".to_string());
    }

    result
}

/// 创建测试指令
fn make_test_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string(instr_type)),
        ("params", JsonValue::object_from_pairs(params)),
    ])
}

/// 热重载管理器
///
/// 监听配置文件变化，通过 watch 通道广播新配置。
pub struct HotReloader {
    /// 配置文件路径
    config_path: PathBuf,
    /// watch 通道发送端（广播新配置）
    tx: watch::Sender<Vec<JsonValue>>,
    /// 文件监听器（保活）
    _watcher: RecommendedWatcher,
}

impl HotReloader {
    /// 创建热重载管理器
    ///
    /// # 参数
    /// - `config_path`：core_eval.json 路径
    ///
    /// # 返回
    /// - `HotReloader` 实例
    /// - `watch::Receiver<Vec<JsonValue>>`：接收端，用于获取最新配置
    pub fn new(
        config_path: PathBuf,
    ) -> Result<(Self, watch::Receiver<Vec<JsonValue>>), HotReloadError> {
        // 初始加载
        let initial_config = load_core_eval(&config_path)?;

        // 验证初始配置
        let validation = validate_core_eval(&initial_config);
        if !validation.valid {
            let errors = validation.errors.join("; ");
            return Err(HotReloadError::ValidationError(errors));
        }
        if !validation.warnings.is_empty() {
            for w in &validation.warnings {
                warn!("Hot reload: config warning - {}", w);
            }
        }

        info!(
            "Hot reload: initial config loaded with {} transforms from {}",
            initial_config.len(),
            config_path.display()
        );

        let (tx, rx) = watch::channel(initial_config);

        // 设置文件监听
        let watch_path = config_path.clone();
        let tx_clone = tx.clone();
        let config_path_for_closure = config_path.clone();

        // notify watcher 回调分支多 (Ok/Err + 多种 Event), 拆函数需闭包捕获。详见 GATE_REFERENCE.md §六(豁免索引)
        #[allow(clippy::cognitive_complexity)]
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // 只处理修改事件
                if matches!(event.kind, EventKind::Modify(_)) {
                    // 稍作延迟避免写入未完成
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    match load_core_eval(&config_path_for_closure) {
                        Ok(new_config) => {
                            // 验证新配置（宪法稳定性：只接受合法配置）
                            let validation = validate_core_eval(&new_config);
                            if !validation.valid {
                                error!("Hot reload: config validation failed, rejecting update");
                                for e in &validation.errors {
                                    error!("Hot reload: validation error - {}", e);
                                }
                                return;
                            }

                            info!(
                                "Hot reload: config reloaded with {} transforms",
                                new_config.len()
                            );
                            if !validation.warnings.is_empty() {
                                for w in &validation.warnings {
                                    warn!("Hot reload: config warning - {}", w);
                                }
                            }

                            if tx_clone.send(new_config).is_err() {
                                warn!("Hot reload: no receivers, config update dropped");
                            }
                        }
                        Err(e) => {
                            error!("Hot reload: failed to reload config: {}", e);
                        }
                    }
                }
            }
        })
        .map_err(|e| HotReloadError::WatchError(e.to_string()))?;

        // 监听配置文件所在目录
        let watch_dir = watch_path.parent().unwrap_or(&watch_path);
        watcher
            .watch(watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| HotReloadError::WatchError(e.to_string()))?;

        info!("Hot reload: watching {}", watch_dir.display());

        Ok((
            Self {
                config_path,
                tx,
                _watcher: watcher,
            },
            rx,
        ))
    }

    /// 获取配置文件路径
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// 手动触发重新加载
    pub fn reload(&self) -> Result<usize, HotReloadError> {
        let config = load_core_eval(&self.config_path)?;
        let count = config.len();
        let _ = self.tx.send(config);
        Ok(count)
    }

    /// 获取当前配置（通过 watch 通道的 borrow）
    pub fn current_config(&self) -> watch::Ref<'_, Vec<JsonValue>> {
        self.tx.borrow()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_core_eval_valid() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_core_eval.json");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(
            br#"{
                "transform": [
                    {"type": "set", "params": {"attr": "x", "value": 1}}
                ]
            }"#,
        )
        .unwrap();

        let result = load_core_eval(&path).unwrap();
        assert_eq!(result.len(), 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_load_core_eval_missing_file() {
        let path = PathBuf::from("/nonexistent/path/file.json");
        let result = load_core_eval(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_core_eval_invalid_json() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_invalid_eval.json");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"not valid json").unwrap();

        let result = load_core_eval(&path);
        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_load_core_eval_no_transform_field() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_no_transform.json");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(br#"{"version": "1.0"}"#).unwrap();

        let result = load_core_eval(&path).unwrap();
        assert!(result.is_empty());
        std::fs::remove_file(path).ok();
    }
}
