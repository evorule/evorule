// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! S5 真机验收样例包派生（env 门控，默认无操作）
//!
//! 从 dist/fixtures 冻结源派生验收样例（正例 + 四类篡改负例，设计稿 §8.2）
//! 落 `dist/acceptance/`，同时跑 handlers 判定层（core 同源 = 正本）落
//! `<样例名>.expected.json` 期望判定留底——真机判定逐条对照此留底
//! （验收标准 §8.1）。派生全程确定性：重跑与已提交样例逐字节一致。
//!
//! 运行：`$env:EVID_WASM_ACCEPTANCE_DIR='<crate>/dist/acceptance';
//!        cargo test -p evid-verify-wasm --test acceptance_samples`

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_wasm::handlers;
use serde_json::{json, Value as Jv};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dist/fixtures");

fn load(name: &str) -> Jv {
    let raw = std::fs::read(format!("{FIXTURES}/{name}.json")).expect("冻结源应存在");
    serde_json::from_slice(&raw).expect("冻结源应可解析")
}

/// hex 串前 8 字符翻改（保持合法 hex，签名自洽格式但验证必败）
fn flip_hex(sig: &str) -> String {
    let mut chars: Vec<char> = sig.chars().collect();
    for c in chars.iter_mut().take(8) {
        *c = if *c == '0' { '1' } else { '0' };
    }
    chars.into_iter().collect()
}

#[test]
// 样例派生为逐样例线性叙事（构造+落盘+断言一体），拆函数反而伤可读性——超行豁免
#[allow(clippy::too_many_lines)]
fn derive_acceptance_samples() {
    let Some(dir) = std::env::var_os("EVID_WASM_ACCEPTANCE_DIR") else {
        return; // 默认无操作（CI 常态），置位才落样例包
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("验收目录应可创建");

    // ---- 导出面：正例 + 三类导出篡改（设计稿 §8.2 前三类）----
    let export = load("export_valid");
    let tamper_fact = {
        let mut d = export.clone();
        d["entries"][1]["fact"]["path"] = json!("tampered.path");
        d
    };
    let tamper_hash = {
        let mut d = export.clone();
        // content_hash 改而不重算：重算失配在 content 步红（链/链尾不动）
        d["entries"][1]["content_hash"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        d
    };
    let reorder = {
        let mut d = export.clone();
        let e0 = d["entries"][0].clone();
        d["entries"][0] = d["entries"][1].clone();
        d["entries"][1] = e0;
        // 首条 prev_hash != genesis → link 步红（重排未重链的必然形态）
        d
    };

    // ---- 锚点面：正例 + 换签名（§8.2 第四类）----
    let seals = load("seals_valid");
    let swapped_sig = {
        let mut d = seals.clone();
        let sig = d["seals"][1]["signature"].as_str().expect("签名应为字符串");
        d["seals"][1]["signature"] = json!(flip_hex(sig));
        d
    };

    // ---- (文件名, 判定面, 样例, 期望极性, 期望首红特征) ----
    type Case<'a> = (&'a str, u8, Jv, bool, Expect);
    enum Expect {
        None,
        Step(usize, &'static str), // stopped_at + 首红 verdict step
        ErrContains(&'static str),
    }
    let cases: Vec<Case> = vec![
        ("01_export_positive", 1, export, true, Expect::None),
        (
            "02_export_neg_tamper_fact",
            1,
            tamper_fact,
            false,
            Expect::Step(1, "content"),
        ),
        (
            "03_export_neg_tamper_content_hash",
            1,
            tamper_hash,
            false,
            Expect::Step(1, "content"),
        ),
        (
            "04_export_neg_reorder",
            1,
            reorder,
            false,
            Expect::Step(0, "link"),
        ),
        ("05_seals_positive", 3, seals, true, Expect::None),
        (
            "06_seals_neg_swapped_signature",
            3,
            swapped_sig,
            false,
            Expect::ErrContains("签名验证失败"),
        ),
    ];

    for (name, face, sample, expect_ok, expect) in cases {
        let verdict = match face {
            1 => handlers::handle_export(&sample),
            2 => handlers::handle_wal(&sample),
            _ => handlers::handle_seals(&sample),
        };
        // 极性自证（正本判定，验收对照的事实基准）
        assert_eq!(verdict["ok"], Jv::Bool(expect_ok), "{name} 极性不符");
        match &expect {
            Expect::Step(stopped, step) => {
                assert_eq!(verdict["stopped_at"], json!(stopped), "{name} 停点不符");
                assert_eq!(
                    verdict["verdicts"][stopped]["step"],
                    json!(step),
                    "{name} 步不符"
                );
                assert_eq!(
                    verdict["verdicts"][stopped]["ok"],
                    Jv::Bool(false),
                    "{name} 首红不符"
                );
            }
            Expect::ErrContains(s) => {
                let err = verdict["error"].as_str().expect("error 应为字符串");
                assert!(err.contains(s), "{name}: 期望含「{s}」，实得 {err}");
            }
            Expect::None => {}
        }
        // 样例与期望判定双双落盘（样例上手机；expected 为正本留底）
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string_pretty(&sample).expect("样例序列化失败"),
        )
        .expect("样例写入失败");
        std::fs::write(
            dir.join(format!("{name}.expected.json")),
            serde_json::to_string_pretty(&verdict).expect("期望序列化失败"),
        )
        .expect("期望写入失败");
    }
}
