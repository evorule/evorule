// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Concurrent session isolation tests — 041 v2.0 A6
//!
//! Verifies the project_memory hard constraint:
//! "each session must have an independent reactor instance to ensure multi-user
//! concurrent isolation".
//!
//! Covers three concurrency scenarios:
//! 1. Multiple sessions execute commands concurrently without state cross-talk
//!    (core isolation guarantee)
//! 2. Concurrent session creation/close with no deadlock, count returns to zero
//! 3. Session IDs created concurrently are unique
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use evorule_governance::SessionManager;
use evorule_reactor::{Fact, FactId};
use evorule_tcb::JsonValue;

/// Builds a transform rule consistent with the core_eval.json increment rule.
///
/// core_eval is the "mapping rule" (branch → set+add), not the business instruction itself.
/// The reactor uses it to transform the business instruction "increment" into the meta
/// instructions set(attr, add, delta).
fn make_core_eval() -> Vec<JsonValue> {
    vec![JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("instruction")),
                        ("instruction_type", JsonValue::string("increment")),
                    ]),
                ),
                (
                    "on_true",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("set")),
                        (
                            "params",
                            JsonValue::object_from_pairs(&[
                                (
                                    "attr",
                                    JsonValue::string("__exec__.instruction.params.attr"),
                                ),
                                ("operation", JsonValue::string("add")),
                                (
                                    "value",
                                    JsonValue::string("__exec__.instruction.params.delta"),
                                ),
                            ]),
                        ),
                    ])]),
                ),
            ]),
        ),
    ])]
}

fn make_increment(fact_id: u64, delta: i64) -> Fact {
    Fact::Command {
        id: FactId(fact_id),
        instruction: JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(delta)),
                ]),
            ),
        ]),
    }
}

/// Read the integer value of the x field from a payload
fn payload_x(payload: &JsonValue) -> Option<i64> {
    match payload {
        JsonValue::Object(map) => map.get("x").and_then(|v| v.as_i64()),
        _ => None,
    }
}

/// 8 sessions concurrently execute 25 increment commands, each session with a different delta.
/// Verifies each session ends with x == delta * 25, no state cross-talk, and each audit
/// chain intact.
#[tokio::test]
async fn concurrent_sessions_state_isolation() {
    const N_SESSIONS: usize = 8;
    const CMDS_PER_SESSION: usize = 25;

    let mgr = Arc::new(SessionManager::new(make_core_eval(), 10000));

    // 1. Create N sessions
    let mut session_ids = Vec::with_capacity(N_SESSIONS);
    for _ in 0..N_SESSIONS {
        session_ids.push(mgr.create_session().expect("create session"));
    }
    assert_eq!(mgr.len(), N_SESSIONS);

    // 2. Concurrent: one task per session, sending CMDS increment(delta=idx+1) commands
    let mut tasks = Vec::with_capacity(N_SESSIONS);
    for (idx, &sid) in session_ids.iter().enumerate() {
        let session = mgr.get_session(sid).expect("get session");
        let delta = (idx + 1) as i64;
        tasks.push(tokio::spawn(async move {
            for n in 1..=CMDS_PER_SESSION {
                session
                    .command_tx
                    .send(make_increment(n as u64, delta))
                    .expect("send command");
            }
            // Expected x = delta * CMDS
            delta * CMDS_PER_SESSION as i64
        }));
    }

    // 3. Wait for all commands to be sent
    let mut expected = Vec::with_capacity(N_SESSIONS);
    for t in tasks {
        expected.push(t.await.expect("task join"));
    }

    // 4. Wait for the reactors to finish processing (long-running mode, they stay alive after Stable)
    tokio::time::sleep(Duration::from_millis(800)).await;

    // 5. Verify isolation: each session x == expected[idx], no cross-talk; each audit chain intact
    for (idx, &sid) in session_ids.iter().enumerate() {
        let session = mgr.get_session(sid).expect("get session after run");
        let (payload, _queue, version) = session.facts_log.snapshot();
        let x = payload_x(&payload)
            .unwrap_or_else(|| panic!("session {} payload is missing the x field: {}", sid, payload));
        assert_eq!(
            x, expected[idx],
            "session {} (idx {}) state cross-talk: x={} expected={} (concurrent isolation broken)",
            sid, idx, x, expected[idx]
        );
        assert!(
            session.audit_verify(),
            "session {} audit-chain integrity verification failed",
            sid
        );
        assert!(
            version >= CMDS_PER_SESSION as u64,
            "session {} version unexpectedly low: {} (expected >= {})",
            sid,
            version,
            CMDS_PER_SESSION
        );
    }

    // 6. Cleanup
    for &sid in &session_ids {
        let _ = mgr.close_session(sid);
    }
}

/// Concurrent session create/close with no deadlock; the count ends at zero.
#[tokio::test]
async fn concurrent_create_close_no_deadlock() {
    let mgr = Arc::new(SessionManager::new(make_core_eval(), 100));

    let mut tasks = Vec::new();
    for _ in 0..16 {
        let mgr = mgr.clone();
        tasks.push(tokio::spawn(async move {
            let id = mgr.create_session().expect("create session");
            // Close immediately (triggers command_tx drop → graceful reactor exit)
            let _handle = mgr.close_session(id).expect("close session");
        }));
    }

    for t in tasks {
        t.await.expect("task join");
    }

    assert_eq!(mgr.len(), 0, "count should be zero after all sessions are closed, got {}", mgr.len());
}

/// Session IDs created concurrently must be unique.
#[tokio::test]
async fn concurrent_create_yields_unique_ids() {
    let mgr = Arc::new(SessionManager::new(make_core_eval(), 200));

    let mut tasks = Vec::new();
    for _ in 0..32 {
        let mgr = mgr.clone();
        tasks.push(tokio::spawn(async move {
            mgr.create_session().expect("create")
        }));
    }

    let mut ids = Vec::new();
    for t in tasks {
        ids.push(t.await.expect("join"));
    }

    ids.sort();
    let original_len = ids.len();
    ids.dedup();
    assert_eq!(
        ids.len(),
        original_len,
        "concurrent creation produced duplicate SessionIds: {} unique / {} total",
        ids.len(),
        original_len
    );

    for id in &ids {
        let _ = mgr.close_session(*id);
    }
}
