// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Mobile golden-vector gate, host side / authority: runs the handlers verdict
//! layer (same source as core = the authority) over the frozen vectors in
//! dist/fixtures and writes outputs to disk for byte-exact comparison against
//! the Node side and the wasm32 artifact.
//!
//! Writes only when `EVID_WASM_HOST_OUT_DIR` is set (no-op by default — CI
//! normal and local full test runs unaffected; comparison script:
//! scripts/evid_wasm_gate.mjs, gate placement: wasm.yml).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_wasm::handlers;
use serde_json::Value as Jv;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dist/fixtures");

/// Fixture file name → verdict surface (matches the abi.rs dispatch face; 1=export 2=WAL 3=anchors)
const FACES: &[(&str, u8)] = &[
    ("export_valid", 1),
    ("export_tampered", 1),
    ("self_check", 1),
    ("wal_valid", 2),
    ("wal_tampered", 2),
    ("seals_valid", 3),
    ("seals_tampered", 3),
];

fn run_face(face: u8, input: &Jv) -> Jv {
    match face {
        1 => handlers::handle_export(input),
        2 => handlers::handle_wal(input),
        _ => handlers::handle_seals(input),
    }
}

#[test]
fn dump_host_expected_outputs() {
    let Some(dir) = std::env::var_os("EVID_WASM_HOST_OUT_DIR") else {
        return; // no-op by default (CI normal); outputs written only when set
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("output directory must be creatable");
    for (name, face) in FACES {
        let raw = std::fs::read(format!("{FIXTURES}/{name}.json")).expect("frozen vector must exist");
        let input: Jv = serde_json::from_slice(&raw).expect("frozen vector must parse");
        let verdict = run_face(*face, &input);
        // Polarity self-proof: valid/self_check must be green, tampered must be red (fixtures themselves proven trustworthy first)
        let expect_ok = !name.contains("tampered");
        assert_eq!(verdict["ok"], Jv::Bool(expect_ok), "{name} polarity mismatch");
        // Compact serialization written byte-exactly (same scheme as the abi.rs package output: verdict.to_string())
        std::fs::write(dir.join(format!("{name}.out.json")), verdict.to_string())
            .expect("expected output write failed");
    }
}
