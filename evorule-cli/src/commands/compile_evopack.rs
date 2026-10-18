// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule compile-evopack` —— host 编译器：JSON 规则 → 紧凑 .evopack 二进制包
//!
//! # 正典编码路径
//! 复用 [`evorule_mcu::RulePack`] 的编码实现（tier0 线格式 + postcard），
//! 与 MCU no_std 内核 `RulePack::from_evopack` 加载侧互为对偶——host 侧与
//! 嵌入式侧共用同一 SSOT，不存在复刻编码器。
//!
//! # 确定性
//! 同输入规则必得同字节：线格式基于 BTreeMap 迭代序（键序字母序确定）+
//! postcard 变长整数编码，无随机因素（由本模块双跑对拍测试与 evorule-mcu
//! fixture 守卫测试双层守护）。

use std::fs;
use std::path::Path;

use evorule_mcu::RulePack;

use crate::error::CliError;
use crate::io_util;

/// 执行 compile-evopack 子命令
pub fn run(rules_file: &Path, output: &Path) -> Result<(), CliError> {
    let rules = io_util::load_rules_file(rules_file)?;
    let rule_count = rules.len();
    let pack = RulePack::from_rules(rules)
        .map_err(|e| CliError::other(format!("cannot compile rules: {e}")))?;
    let bytes = pack
        .to_evopack()
        .map_err(|e| CliError::other(format!("cannot encode evopack: {e}")))?;

    fs::write(output, &bytes)?;

    println!("=== Compiling evopack ===");
    println!("Rules:  {}", rule_count);
    println!("Output: {} ({} bytes)", output.display(), bytes.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use std::fs;

    fn make_temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("evorule-cli-pack-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    /// 两条规则的样例：branch（domain 门控）+ 直接 set
    const SAMPLE_RULES: &str = r#"[
        {
            "type": "branch",
            "params": {
                "domain": {"type": "instruction", "instruction_type": "increment"},
                "on_true": [
                    {"type": "set", "params": {"attr": "x", "operation": "add",
                     "value": "__exec__.instruction.params.delta"}}
                ]
            }
        },
        {"type": "set", "params": {"attr": "y", "operation": "set", "value": 7}}
    ]"#;

    #[test]
    fn test_compile_deterministic_same_bytes() {
        // T4 验收判据：确定性——同输入两跑字节相同
        let dir = make_temp_dir("deterministic");
        let rules_file = dir.join("rules.json");
        fs::write(&rules_file, SAMPLE_RULES).unwrap();

        let out_a = dir.join("a.evopack");
        let out_b = dir.join("b.evopack");
        run(&rules_file, &out_a).unwrap();
        run(&rules_file, &out_b).unwrap();

        let bytes_a = fs::read(&out_a).unwrap();
        let bytes_b = fs::read(&out_b).unwrap();
        assert!(!bytes_a.is_empty());
        assert_eq!(bytes_a, bytes_b, "同输入必须同字节（确定性）");

        // 产物可被 MCU 内核加载侧解码（host 编译器与内核加载侧互为对偶）
        let pack = RulePack::from_evopack(&bytes_a).unwrap();
        assert_eq!(pack.rules().len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn test_compile_rejects_rule_count_over_limit() {
        // 规则数超过 TCB 上限（MAX_TRANSFORM_RULES=64）：构造期拒绝（fail-fast）
        let dir = make_temp_dir("over-limit");
        let rules_file = dir.join("rules.json");
        let mut json = String::from("[");
        for i in 0..65 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"type":"set","params":{{"attr":"x","operation":"set","value":{i}}}}}"#
            ));
        }
        json.push(']');
        fs::write(&rules_file, json).unwrap();

        let out = dir.join("out.evopack");
        let err = run(&rules_file, &out).unwrap_err();
        assert!(
            err.to_string().contains("exceeds transform limit"),
            "got: {err}"
        );
        assert!(!out.exists(), "编译失败不得产出文件");

        cleanup(&dir);
    }

    #[test]
    fn test_compile_invalid_json_fails() {
        let dir = make_temp_dir("bad-json");
        let rules_file = dir.join("bad.json");
        fs::write(&rules_file, "not json").unwrap();

        let out = dir.join("out.evopack");
        let err = run(&rules_file, &out).unwrap_err();
        assert!(err.to_string().contains("Invalid JSON"), "got: {err}");

        cleanup(&dir);
    }
}
