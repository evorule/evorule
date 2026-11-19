// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! route B: `#[wasm_bindgen]` binding surface (wasm-pack / npm ecosystem)
//!
//! Enabled with `--features bindgen` (not in the default build —
//! wasm-bindgen attaches host imports into the module, breaking route A's
//! zero-import install-free instantiation). Host (non-wasm32) targets compile
//! and test equally; verdict logic identical to [`crate::handlers`], strings
//! in and out.

use serde_json::{json, Value as Jv};
use wasm_bindgen::prelude::*;

use crate::handlers;

fn run(input: &str, f: impl Fn(&Jv) -> Jv) -> String {
    match serde_json::from_str::<Jv>(input) {
        Ok(v) => f(&v).to_string(),
        Err(e) => json!({ "ok": false, "error": format!("invalid input JSON: {e}") }).to_string(),
    }
}

/// Export-surface verdict (JSON strings in and out)
#[wasm_bindgen]
pub fn verify_export_json(input: &str) -> String {
    run(input, handlers::handle_export)
}

/// WAL-surface verdict (JSON strings in and out)
#[wasm_bindgen]
pub fn verify_wal_json(input: &str) -> String {
    run(input, handlers::handle_wal)
}

/// Anchor-surface verdict (JSON strings in and out)
#[wasm_bindgen]
pub fn verify_seals_json(input: &str) -> String {
    run(input, handlers::handle_seals)
}
