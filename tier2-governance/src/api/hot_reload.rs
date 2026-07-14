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
use tier0_tcb::JsonValue;
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

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // 只处理修改事件
                if matches!(event.kind, EventKind::Modify(_)) {
                    // 稍作延迟避免写入未完成
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    match load_core_eval(&config_path_for_closure) {
                        Ok(new_config) => {
                            info!(
                                "Hot reload: config reloaded with {} transforms",
                                new_config.len()
                            );
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
