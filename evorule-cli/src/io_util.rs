// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O 工具：规则加载 + payload 解析 + 文件读写
//!
//! # P0-2 修复：确定性加载
//! `load_rules` 必须按文件名排序后加载，保证不同平台/文件系统返回顺序一致。
//! 原实现依赖 `fs::read_dir` 的返回顺序（Windows NTFS 字典序、Linux ext4 hash 序），
//! 导致同目录规则在不同平台执行结果不同。
//!
//! # 复用 evorule-reactor 序列化
//! `serde_json::Value → evorule_tcb::JsonValue` 转换复用 `evorule_reactor::serde_to_tcb`，
//! 保证与 tier1 reactor WAL、tier2 auditor 的序列化路径一致。

use std::fs;
use std::path::Path;

use evorule_tcb::JsonValue;
use serde_json::Value as SerdeValue;

use crate::error::CliError;

/// 加载规则目录，合并 transform 列表
///
/// # 确定性加载（P0-2 修复）
/// 按 `file_name()` 字典序排序后加载，消除 `fs::read_dir` 顺序差异。
///
/// # 支持的文件格式
/// 每个 `.json` 文件可以是以下三种格式之一：
/// 1. `{"transform": [...]}` 或 `{"transforms": [...]}` — 提取数组
/// 2. `[{...}, {...}]` — 顶层数组，每项是一条 transform
/// 3. `{...}` — 单条 transform 对象
///
/// # 保留数据文件排除
/// 文件名恰好为 `payload.json`（大小写不敏感）的文件被当作**初始输入数据**而非规则，
/// 不参与加载。若用户在规则目录内放置初始 payload，其 `{}` 通常无 `type` 字段，
/// 会被误当成规则并触发 `missing field: type`（见 `parse_initial_payload` 与教程约定）。
///
/// # 错误
/// - `RulesDirNotFound`：目录不存在
/// - `NoRulesFound`：目录中无 `.json` 文件
/// - `Io`：读取文件失败
/// - `Json`：JSON 解析失败
pub fn load_rules(rules_dir: &Path) -> Result<Vec<JsonValue>, CliError> {
    if !rules_dir.exists() {
        return Err(CliError::RulesDirNotFound(rules_dir.display().to_string()));
    }
    if !rules_dir.is_dir() {
        return Err(CliError::RulesDirNotFound(format!(
            "Not a directory: {}",
            rules_dir.display()
        )));
    }

    // 收集 .json 文件并按文件名排序（P0-2 修复）
    // 排除保留数据文件 `payload.json`（大小写不敏感）：它通常无 `type` 字段，
    // 若被当规则加载会触发 "missing field: type"（见 parse_initial_payload 与教程约定）。
    let mut entries: Vec<_> = fs::read_dir(rules_dir)?
        .filter_map(Result::ok)
        .filter(|e| {
            let is_json = e
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext == "json");
            let is_payload = e
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("payload.json"));
            is_json && !is_payload
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        return Err(CliError::NoRulesFound(rules_dir.display().to_string()));
    }

    let mut all_transforms: Vec<SerdeValue> = Vec::new();
    for entry in &entries {
        let path = entry.path();
        let content = fs::read_to_string(&path)?;
        let json: SerdeValue = serde_json::from_str(&content)
            .map_err(|e| CliError::Other(format!("Invalid JSON in {}: {}", path.display(), e)))?;
        all_transforms.extend(extract_transforms(json));
    }

    tracing::info!(
        files = entries.len(),
        transforms = all_transforms.len(),
        "Rules loaded"
    );

    // 复用 evorule-reactor 序列化：serde_json::Value → tier0 JsonValue
    Ok(all_transforms
        .into_iter()
        .map(|v| evorule_reactor::serde_to_tcb(&v))
        .collect())
}

/// 从单个 JSON 文件提取 transform 列表
///
/// 支持三格式：`{transform: [...]}` / `{transforms: [...]}` / 顶层数组 / 单对象
fn extract_transforms(json: SerdeValue) -> Vec<SerdeValue> {
    match &json {
        SerdeValue::Object(map) => {
            if let Some(SerdeValue::Array(arr)) = map.get("transform") {
                arr.clone()
            } else if let Some(SerdeValue::Array(arr)) = map.get("transforms") {
                arr.clone()
            } else {
                vec![json.clone()]
            }
        }
        SerdeValue::Array(arr) => arr.clone(),
        _ => vec![json.clone()],
    }
}

/// 解析初始 payload
///
/// 优先级：`--payload` 字符串 > `--payload-file` 文件 > 默认空对象 `{}`
///
/// # 错误
/// - `InvalidPayload`：JSON 解析失败
pub fn parse_initial_payload(
    payload_str: Option<&str>,
    payload_file: Option<&Path>,
) -> Result<JsonValue, CliError> {
    let raw: Option<String> = match (payload_str, payload_file) {
        (Some(s), _) => Some(s.to_string()),
        (None, Some(path)) => Some(fs::read_to_string(path).map_err(|e| {
            CliError::Other(format!(
                "Failed to read payload file {}: {}",
                path.display(),
                e
            ))
        })?),
        (None, None) => None,
    };

    match raw {
        Some(s) => {
            let json: SerdeValue =
                serde_json::from_str(&s).map_err(|e| CliError::InvalidPayload(e.to_string()))?;
            Ok(evorule_reactor::serde_to_tcb(&json))
        }
        None => Ok(JsonValue::empty_object()),
    }
}

/// 写输出到文件或 stdout
///
/// `output` 为 `None` 时打印到 stdout，为 `Some(path)` 时写入文件。
pub fn write_output(output: Option<&Path>, content: &str) -> Result<(), CliError> {
    match output {
        Some(path) => {
            fs::write(path, content)?;
            tracing::info!(path = %path.display(), "Output written");
        }
        None => println!("{}", content),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::fs;
    use std::io::Write;

    fn make_temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("evorule-cli-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_load_rules_deterministic_order() {
        // 创建 3 个文件，文件名顺序与写入顺序故意不一致
        let dir = make_temp_dir("deterministic");
        let write_rule = |name: &str, t: &str| {
            let mut f = fs::File::create(dir.join(name)).unwrap();
            f.write_all(t.as_bytes()).unwrap();
        };
        write_rule(
            "03-third.json",
            r#"{"transform":[{"type":"set","params":{}}]}"#,
        );
        write_rule("01-first.json", r#"{"transform":[{"type":"noop"}]}"#);
        write_rule(
            "02-second.json",
            r#"{"transform":[{"type":"set","params":{}}]}"#,
        );

        let rules1 = load_rules(&dir).unwrap();
        let rules2 = load_rules(&dir).unwrap();

        assert_eq!(rules1.len(), 3, "should load 3 transforms from 3 files");
        assert_eq!(
            rules1, rules2,
            "load_rules must be deterministic across calls"
        );

        // 验证顺序：01-first 的 noop 应该在第一位
        let first = &rules1[0];
        let type_str = first.get("type").and_then(|v| v.as_str()).unwrap();
        assert_eq!(type_str, "noop");

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_three_formats() {
        let dir = make_temp_dir("formats");
        // 格式1: {transform: [...]}
        fs::write(dir.join("a.json"), r#"{"transform":[{"type":"noop"}]}"#).unwrap();
        // 格式2: {transforms: [...]}
        fs::write(dir.join("b.json"), r#"{"transforms":[{"type":"noop"}]}"#).unwrap();
        // 格式3: 顶层数组
        fs::write(dir.join("c.json"), r#"[{"type":"noop"}]"#).unwrap();
        // 格式4: 单对象
        fs::write(dir.join("d.json"), r#"{"type":"noop"}"#).unwrap();

        let rules = load_rules(&dir).unwrap();
        assert_eq!(rules.len(), 4, "should load 4 transforms from 4 formats");

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_ignores_payload_json() {
        let dir = make_temp_dir("payload-skip");
        // 规则文件
        fs::write(dir.join("01-capture.json"), r#"{"transform":[{"type":"noop"}]}"#).unwrap();
        // 保留数据文件：无 type 字段的初始 payload，不应被当作规则加载
        fs::write(dir.join("payload.json"), r#"{"request_id":"REQ-001"}"#).unwrap();

        let rules = load_rules(&dir).unwrap();
        assert_eq!(
            rules.len(),
            1,
            "payload.json must not be loaded as a rule, got {} transforms",
            rules.len()
        );
        assert_eq!(
            rules[0].get("type").and_then(|v| v.as_str()),
            Some("noop"),
            "only the real rule should be loaded"
        );

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_dir_not_found() {
        let result = load_rules(std::path::Path::new("/nonexistent/path/xyz"));
        assert!(matches!(result, Err(CliError::RulesDirNotFound(_))));
    }

    #[test]
    fn test_load_rules_no_json() {
        let dir = make_temp_dir("empty");
        fs::write(dir.join("readme.txt"), "not a rule").unwrap();
        let result = load_rules(&dir);
        assert!(matches!(result, Err(CliError::NoRulesFound(_))));
        cleanup(&dir);
    }

    #[test]
    fn test_parse_initial_payload_default() {
        let payload = parse_initial_payload(None, None).unwrap();
        assert_eq!(payload, JsonValue::empty_object());
    }

    #[test]
    fn test_parse_initial_payload_from_string() {
        let payload = parse_initial_payload(Some(r#"{"x": 42}"#), None).unwrap();
        assert_eq!(payload.get("x").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn test_parse_initial_payload_invalid() {
        let result = parse_initial_payload(Some("not json"), None);
        assert!(matches!(result, Err(CliError::InvalidPayload(_))));
    }
}
