// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O utilities: rule loading + payload parsing + file read/write
//!
//! # P0-2 fix: deterministic loading
//! `load_rules` must sort by file name before loading, guaranteeing a consistent
//! order across platforms/filesystems. The original implementation depended on
//! `fs::read_dir` return order (Windows NTFS lexicographic, Linux ext4 hash
//! order), which made the same rule directory behave differently on different
//! platforms.
//!
//! # Reuses the evorule-reactor serialization
//! The `serde_json::Value → evorule_tcb::JsonValue` conversion reuses
//! `evorule_reactor::serde_to_tcb`, keeping the serialization path identical to
//! the tier1 reactor WAL and the tier2 auditor.

use std::fs;
use std::path::Path;

use evorule_tcb::JsonValue;
use serde_json::Value as SerdeValue;

use crate::error::CliError;

/// Load a rule directory, merging the transform list
///
/// # Deterministic loading (P0-2 fix)
/// Load after sorting by `file_name()` lexicographic order, eliminating
/// `fs::read_dir` order differences.
///
/// # Supported file formats
/// Each `.json` file can be one of three formats:
/// 1. `{"transform": [...]}` or `{"transforms": [...]}` — extract the array
/// 2. `[{...}, {...}]` — top-level array, each item a transform
/// 3. `{...}` — a single transform object
///
/// # Reserved data file exclusion
/// A file named exactly `payload.json` (case-insensitive) is treated as
/// **initial input data**, not a rule, and is not loaded. If a user places an
/// initial payload inside the rules directory, its `{}` usually has no `type`
/// field and would be mistaken for a rule, triggering `missing field: type`
/// (see `parse_initial_payload` and the tutorial conventions).
///
/// # Errors
/// - `RulesDirNotFound`: directory does not exist
/// - `NoRulesFound`: no `.json` files in the directory
/// - `Io`: failed to read a file
/// - `Json`: JSON parse failure
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

    // Collect .json files and sort by file name (P0-2 fix)
    // Exclude the reserved data file `payload.json` (case-insensitive): it usually has
    // no `type` field, and loading it as a rule would trigger "missing field: type"
    // (see parse_initial_payload and the tutorial conventions).
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

    // Reuse the evorule-reactor serialization: serde_json::Value → tier0 JsonValue
    Ok(all_transforms
        .into_iter()
        .map(|v| evorule_reactor::serde_to_tcb(&v))
        .collect())
}

/// Load a single rule file (used by the compile-evopack host compiler)
///
/// Isomorphic to the single-file side of [`load_rules`]: supports the three
/// formats (`{"transform": [...]}` / `{"transforms": [...]}` / top-level array /
/// single object), reuses [`extract_transforms`] for extraction plus the
/// `serde_to_tcb` conversion, keeping the serialization semantics consistent
/// with the rule-directory loading path.
///
/// # Errors
/// - `Io`: failed to read the file
/// - `Other`: JSON parse failure (with file path context)
pub fn load_rules_file(rules_file: &Path) -> Result<Vec<JsonValue>, CliError> {
    let content = fs::read_to_string(rules_file)?;
    let json: SerdeValue = serde_json::from_str(&content)
        .map_err(|e| CliError::Other(format!("Invalid JSON in {}: {}", rules_file.display(), e)))?;
    Ok(extract_transforms(json)
        .iter()
        .map(evorule_reactor::serde_to_tcb)
        .collect())
}

/// Extract the transform list from a single JSON file
///
/// Supports three formats: `{transform: [...]}` / `{transforms: [...]}` / top-level array / single object
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

/// Parse the initial payload
///
/// Priority: `--payload` string > `--payload-file` file > default empty object `{}`
///
/// # Errors
/// - `InvalidPayload`: JSON parse failure
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

/// Write output to a file or stdout
///
/// `output` being `None` prints to stdout; `Some(path)` writes to the file.
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
        // Create 3 files whose name order deliberately differs from the write order
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

        // Verify order: 01-first's noop should be first
        let first = &rules1[0];
        let type_str = first.get("type").and_then(|v| v.as_str()).unwrap();
        assert_eq!(type_str, "noop");

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_three_formats() {
        let dir = make_temp_dir("formats");
        // Format 1: {transform: [...]}
        fs::write(dir.join("a.json"), r#"{"transform":[{"type":"noop"}]}"#).unwrap();
        // Format 2: {transforms: [...]}
        fs::write(dir.join("b.json"), r#"{"transforms":[{"type":"noop"}]}"#).unwrap();
        // Format 3: top-level array
        fs::write(dir.join("c.json"), r#"[{"type":"noop"}]"#).unwrap();
        // Format 4: single object
        fs::write(dir.join("d.json"), r#"{"type":"noop"}"#).unwrap();

        let rules = load_rules(&dir).unwrap();
        assert_eq!(rules.len(), 4, "should load 4 transforms from 4 formats");

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_ignores_payload_json() {
        let dir = make_temp_dir("payload-skip");
        // A rule file
        fs::write(
            dir.join("01-capture.json"),
            r#"{"transform":[{"type":"noop"}]}"#,
        )
        .unwrap();
        // Reserved data file: an initial payload without a type field, must not be loaded as a rule
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
    fn test_load_rules_file_formats() {
        // Isomorphic with load_rules: a single file supports the transform/transforms/top-level array/single object shapes
        let dir = make_temp_dir("single-file");
        let write_and_load = |name: &str, content: &str| {
            let path = dir.join(name);
            fs::write(&path, content).unwrap();
            load_rules_file(&path).unwrap()
        };

        let t = write_and_load("a.json", r#"{"transform":[{"type":"noop"}]}"#);
        assert_eq!(t.len(), 1);
        let t = write_and_load("b.json", r#"{"transforms":[{"type":"noop"}]}"#);
        assert_eq!(t.len(), 1);
        let t = write_and_load("c.json", r#"[{"type":"noop"},{"type":"noop"}]"#);
        assert_eq!(t.len(), 2);
        let t = write_and_load("d.json", r#"{"type":"noop"}"#);
        assert_eq!(t.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_file_invalid_json() {
        let dir = make_temp_dir("single-file-bad");
        let path = dir.join("bad.json");
        fs::write(&path, "not json").unwrap();
        let result = load_rules_file(&path);
        assert!(
            matches!(&result, Err(CliError::Other(msg)) if msg.contains("Invalid JSON")),
            "got: {result:?}"
        );
        cleanup(&dir);
    }

    #[test]
    fn test_load_rules_file_not_found() {
        let result = load_rules_file(std::path::Path::new("/nonexistent/xyz.json"));
        assert!(matches!(result, Err(CliError::Io(_))));
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
