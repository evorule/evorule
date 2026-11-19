// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Verdict-surface handler: input JSON → verdict JSON (shared by both routes, never panics)
//!
//! Parse failures also produce a structured red (fail-closed shape straight to
//! the UI); all verdict semantics are delegated to [`evid_verify_core`] — this
//! layer only shapes the JSON messages.

use std::collections::BTreeMap;

use evid_verify_core::record::{parse_export_doc, parse_wal_line};
use evid_verify_core::seal::{verify_seal_chain, AnchorSeal};
use evid_verify_core::verify::{verify_export, verify_wal_lines, ExportStep};
use serde_json::{json, Value as Jv};

/// Export surface: parsing (v1.0 fail-closed) + four-step verdict + tail reconciliation
pub fn handle_export(input: &Jv) -> Jv {
    let doc = match parse_export_doc(input) {
        Ok(doc) => doc,
        Err(e) => return json!({ "ok": false, "error": e.reason() }),
    };
    let report = verify_export(&doc);
    let verdicts: Vec<Jv> = report
        .verdicts
        .iter()
        .map(|v| {
            json!({
                "index": v.index,
                "ok": v.ok,
                "step": v.reject.as_ref().map(|r| step_name(r.step)),
                "reason": v.reject.as_ref().map(|r| r.reason.clone()),
            })
        })
        .collect();
    json!({
        "ok": report.all_ok(),
        "verdicts": verdicts,
        "stopped_at": report.stopped_at,
        "tail_hash": report.tail_hash,
        "last_hash_matches": report.last_hash_matches,
    })
}

fn step_name(s: ExportStep) -> &'static str {
    match s {
        ExportStep::Link => "link",
        ExportStep::Content => "content",
        ExportStep::Stable => "stable",
        ExportStep::Tail => "tail",
    }
}

/// WAL surface: parsing + three-step verdict (input accepts `{"lines":[…]}` or a bare array)
pub fn handle_wal(input: &Jv) -> Jv {
    const SHAPE: &str = "WAL input must be an array or {\"lines\":[…]}";
    let arr: &Vec<Jv> = match input {
        Jv::Array(a) => a,
        Jv::Object(o) => match o.get("lines").and_then(Jv::as_array) {
            Some(a) => a,
            None => return json!({ "ok": false, "error": SHAPE }),
        },
        _ => return json!({ "ok": false, "error": SHAPE }),
    };
    let mut lines = Vec::with_capacity(arr.len());
    for (i, lv) in arr.iter().enumerate() {
        match parse_wal_line(lv) {
            Ok(line) => lines.push(line),
            Err(e) => {
                return json!({ "ok": false, "error": format!("lines[{i}]: {}", e.reason()) });
            }
        }
    }
    let report = verify_wal_lines(&lines);
    let verdicts: Vec<Jv> = report
        .verdicts
        .iter()
        .map(|v| {
            json!({
                "index": v.index,
                "ok": v.ok,
                "step": v.reject.as_ref().map(|r| r.step),
                "reason": v.reject.as_ref().map(|r| r.reason.clone()),
            })
        })
        .collect();
    json!({
        "ok": report.all_ok(),
        "verdicts": verdicts,
        "stopped_at": report.stopped_at,
        "tail_hash": report.tail_hash,
    })
}

/// Anchor surface: parsing + four-rule chain verification (public keys injected by key_id lookup, zero hardcoded trust root)
pub fn handle_seals(input: &Jv) -> Jv {
    let Some(obj) = input.as_object() else {
        return json!({ "ok": false, "error": "seals input must be an object {\"seals\":[…], \"keys\":{…}}" });
    };
    let Some(seals_v) = obj.get("seals") else {
        return json!({ "ok": false, "error": "missing field \"seals\"" });
    };
    let seals: Vec<AnchorSeal> = match serde_json::from_value(seals_v.clone()) {
        Ok(s) => s,
        Err(e) => return json!({ "ok": false, "error": format!("seals parse failed: {e}") }),
    };
    let Some(keys_v) = obj.get("keys") else {
        return json!({ "ok": false, "error": "missing field \"keys\" (public key table)" });
    };
    let keys: BTreeMap<String, String> = match serde_json::from_value(keys_v.clone()) {
        Ok(k) => k,
        Err(e) => return json!({ "ok": false, "error": format!("keys parse failed: {e}") }),
    };
    match verify_seal_chain(&seals, &keys) {
        Ok(()) => json!({ "ok": true, "seal_count": seals.len() }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}
