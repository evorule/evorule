// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Real-device acceptance sample package derivation (env-gated, no-op by default)
//!
//! Derives acceptance samples from the frozen sources in dist/fixtures
//! (positives + four tamper-negative classes, design doc §8.2) into
//! `dist/acceptance/`, while running the handlers verdict layer (same source
//! as core = the authority) to write `<sample>.expected.json` expected verdicts
//! for the record — real-device verdicts are checked one by one against this
//! record (acceptance criteria §8.1). Derivation is fully deterministic:
//! re-runs are byte-identical to the committed samples.
//!
//! Run: `$env:EVID_WASM_ACCEPTANCE_DIR='<crate>/dist/acceptance';
//!        cargo test -p evid-verify-wasm --test acceptance_samples`

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_wasm::handlers;
use serde_json::{json, Value as Jv};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dist/fixtures");

fn load(name: &str) -> Jv {
    let raw = std::fs::read(format!("{FIXTURES}/{name}.json")).expect("frozen source must exist");
    serde_json::from_slice(&raw).expect("frozen source must parse")
}

/// Flip the first 8 characters of a hex string (stays valid hex; signature-shape-consistent but verification always fails)
fn flip_hex(sig: &str) -> String {
    let mut chars: Vec<char> = sig.chars().collect();
    for c in chars.iter_mut().take(8) {
        *c = if *c == '0' { '1' } else { '0' };
    }
    chars.into_iter().collect()
}

#[test]
// Sample derivation is a per-sample linear narrative (construction + write + assertion in one body); splitting it would hurt readability — over-length exemption
#[allow(clippy::too_many_lines)]
fn derive_acceptance_samples() {
    let Some(dir) = std::env::var_os("EVID_WASM_ACCEPTANCE_DIR") else {
        return; // no-op by default (CI normal); samples written only when set
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("acceptance directory must be creatable");

    // ---- Export surface: positive + three export tamper classes (design doc §8.2, first three) ----
    let export = load("export_valid");
    let tamper_fact = {
        let mut d = export.clone();
        d["entries"][1]["fact"]["path"] = json!("tampered.path");
        d
    };
    let tamper_hash = {
        let mut d = export.clone();
        // content_hash altered without recomputation: the recomputation mismatch goes red at the content step (chain / tail untouched)
        d["entries"][1]["content_hash"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        d
    };
    let reorder = {
        let mut d = export.clone();
        let e0 = d["entries"][0].clone();
        d["entries"][0] = d["entries"][1].clone();
        d["entries"][1] = e0;
        // First entry prev_hash != genesis → red at the link step (the inevitable shape of reordering without re-folding)
        d
    };

    // ---- Anchor surface: positive + swapped signature (§8.2, fourth class) ----
    let seals = load("seals_valid");
    let swapped_sig = {
        let mut d = seals.clone();
        let sig = d["seals"][1]["signature"]
            .as_str()
            .expect("signature must be a string");
        d["seals"][1]["signature"] = json!(flip_hex(sig));
        d
    };

    // ---- (file name, verdict surface, sample, expected polarity, expected first-red trait) ----
    type Case<'a> = (&'a str, u8, Jv, bool, Expect);
    enum Expect {
        None,
        Step(usize, &'static str), // stopped_at + first-red verdict step
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
            Expect::ErrContains("签名验证失败"), // cross-file anchor on the seal.rs Display message, frozen in dist/acceptance — kept verbatim
        ),
    ];

    for (name, face, sample, expect_ok, expect) in cases {
        let verdict = match face {
            1 => handlers::handle_export(&sample),
            2 => handlers::handle_wal(&sample),
            _ => handlers::handle_seals(&sample),
        };
        // Polarity self-proof (authoritative verdicts; the factual baseline for acceptance comparison)
        assert_eq!(
            verdict["ok"],
            Jv::Bool(expect_ok),
            "{name} polarity mismatch"
        );
        match &expect {
            Expect::Step(stopped, step) => {
                assert_eq!(
                    verdict["stopped_at"],
                    json!(stopped),
                    "{name} stop point mismatch"
                );
                assert_eq!(
                    verdict["verdicts"][stopped]["step"],
                    json!(step),
                    "{name} step mismatch"
                );
                assert_eq!(
                    verdict["verdicts"][stopped]["ok"],
                    Jv::Bool(false),
                    "{name} first-red mismatch"
                );
            }
            Expect::ErrContains(s) => {
                let err = verdict["error"].as_str().expect("error must be a string");
                assert!(
                    err.contains(s),
                    "{name}: expected to contain \"{s}\", got {err}"
                );
            }
            Expect::None => {}
        }
        // Write both the sample and the expected verdict (sample goes to the phone; expected is the authoritative record)
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string_pretty(&sample).expect("sample serialization failed"),
        )
        .expect("sample write failed");
        std::fs::write(
            dir.join(format!("{name}.expected.json")),
            serde_json::to_string_pretty(&verdict).expect("expected serialization failed"),
        )
        .expect("expected write failed");
    }
}
