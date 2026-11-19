// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Differential tests: audit-chain version consistency (P0-9) + time machine vs FactsLog
//! reconstruction consistency (P0-10)
//!
//! # Location
//!
//! This file lives in the `verification/` directory (dedicated to formal verification),
//! decoupled from the core implementation in `src/`:
//! - Not subject to the `build.rs` compile-time gate (which only scans `src/`)
//! - Wired as an integration test via the `[[test]]` target in `Cargo.toml`
//!
//! # Verification goals
//!
//! ## P0-9: `diff_version_consistency`
//!
//! `facts_log.version() == reactor.snapshot().version`
//!
//! Verifies that the audit chain's (FactsLog) version number matches the reactor's internal
//! state version number. If the two diverge, a time-travel debugger would see a version
//! that does not match the actually executed version.
//!
//! ## P0-10: `diff_rewind_vs_factslog`
//!
//! `rewind(facts, N).snapshot == FactsLog replayed up to version N's snapshot`
//!
//! Verifies that the time machine (pure-function reconstruction) and FactsLog (imperative
//! append) produce the same historical snapshots. Two independent paths reaching the same
//! version cross-validate each other.
//!
//! ## P0-15: `diff_recover_vs_append`
//!
//! `FactsLog entry-by-entry append (in-memory WAL backend) final state ==
//!  replay_wal_records(WAL records) final state`
//!
//! Verifies that the in-process append path and the WAL replay path (the pure core of
//! recover) produce the same snapshot/queue/version/tail-hash — audit-chain replay
//! determinism.
//!
//! # Semantic-equivalence specification (formal contract of the differential tests)
//!
//! - **P0-9**: for any instruction sequence `Cmds` and initial payload `S`:
//!   `facts_log.version() == reactor.snapshot().version`
//!   (the audit-chain view and the reactor-internal view of the version number must
//!   agree, never diverge).
//! - **P0-10**: for any fact sequence `Facts` and target version `N`:
//!   `rewind(Facts, N).snapshot == FactsLog replayed through the first N facts' snapshot`
//!   (the version number and payload produced by the two independent reconstruction
//!   paths must be structurally equal).
//! - **P0-15**: for any fact sequence `Facts`:
//!   `append-chain final state == replay_wal_records(records produced by append) final state`
//!   (the snapshot/queue/version/tail-hash quadruple must be fully equal).
//!
//! # How to run
//!
//! ```bash
//! cargo test --package evorule-governance --test differential_test
//! PROPTEST_CASES=1000 cargo test --package evorule-governance --test differential_test
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_governance::time_machine::rewind;
use evorule_reactor::{Fact, FactId, FactsLog, Reactor};
use evorule_tcb::JsonValue;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

// =============================================================================
// Helpers: JsonValue ↔ serde_json::Value conversion (for comparison)
// =============================================================================

/// Convert an evorule_tcb::JsonValue into a serde_json::Value (for comparison with RewindSnapshot)
fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        JsonValue::U64(v) => serde_json::Value::Number((*v).into()),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), tcb_to_serde(v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

// =============================================================================
// Helpers: build Facts
// =============================================================================

fn payload_update_fact(id: u64, path: &str, value: i64) -> Fact {
    Fact::PayloadUpdate {
        id: FactId(id),
        path: path.to_string(),
        value: JsonValue::Integer(value),
    }
}

fn state_transition_fact(id: u64, cause: u64, payload_x: i64, payload_y: i64) -> Fact {
    Fact::StateTransition {
        id: FactId(id),
        cause: FactId(cause),
        new_payload: JsonValue::object_from_pairs(&[
            ("x", JsonValue::Integer(payload_x)),
            ("y", JsonValue::Integer(payload_y)),
        ]),
        new_queue: vec![],
    }
}

// =============================================================================
// proptest configuration
// =============================================================================

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

// =============================================================================
// P0-9: facts_log.version() == reactor.snapshot().version
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// P0-9: after a single Command, the audit-chain version and the reactor-internal
    /// version agree
    ///
    /// Differential pair:
    /// - A: FactsLog::version() (audit-chain view)
    /// - B: ReactorHandle::snapshot().version (reactor-internal view)
    ///
    /// After sending one Command → StateTransition, both versions should be 1.
    #[test]
    fn diff_version_consistency_single_command(
        initial_x in -1_000_000i64..1_000_000,
        new_value in -1_000_000i64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            // Empty core rule set — execute_transition returns State (payload unchanged)
            let core_eval: Vec<JsonValue> = vec![];
            let payload = JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(initial_x)),
                ("y", JsonValue::Integer(0)),
            ]);
            let facts_log = FactsLog::with_initial_payload(payload);
            let reactor = Reactor::builder(core_eval)
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, log) = reactor.spawn();

            // Send a set instruction (triggers StateTransition)
            let instruction = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                ("params", JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(new_value)),
                ])),
            ]);
            // FactSender is an UnboundedSender; send is a synchronous method
            tx.send(Fact::Command {
                id: FactId(1),
                instruction,
            }).expect("send command failed");

            // Wait for Stable (ensures the StateTransition has been recorded by FactsLog)
            let mut got_transition = false;
            while let Ok(fact) = rx.recv().await {
                match fact {
                    Fact::StateTransition { .. } => {
                        got_transition = true;
                    }
                    Fact::Stable { .. } => break,
                    Fact::Error { .. } => break,
                    _ => {}
                }
            }

            handle.abort();

            // Core assertion: if a StateTransition occurred, the version numbers must agree
            if got_transition {
                let reactor_version = handle.snapshot()
                    .expect("reactor snapshot should exist")
                    .version;
                let log_version = log.version();
                prop_assert_eq!(
                    reactor_version, log_version,
                    "version mismatch: reactor={} vs facts_log={} (after 1 StateTransition)",
                    reactor_version, log_version,
                );
            }

            Ok(())
        })?;
    }

    /// P0-9: after multiple Commands, the audit-chain version and the reactor-internal
    /// version agree
    #[test]
    fn diff_version_consistency_multi_command(
        n_commands in 1usize..10,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval: Vec<JsonValue> = vec![];
            let payload = JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(0)),
            ]);
            let facts_log = FactsLog::with_initial_payload(payload);
            let reactor = Reactor::builder(core_eval)
                .max_rounds(1000)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, log) = reactor.spawn();

            // Send n_commands noop instructions
            for i in 0..n_commands {
                let instruction = JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("noop")),
                ]);
                tx.send(Fact::Command {
                    id: FactId(i as u64 + 1),
                    instruction,
                }).expect("send command failed");

                // Wait for this command's Stable
                while let Ok(fact) = rx.recv().await {
                    if matches!(fact, Fact::Stable { .. } | Fact::Error { .. }) {
                        break;
                    }
                }
            }

            handle.abort();

            // Compare version numbers
            let reactor_version = handle.snapshot()
                .expect("reactor snapshot should exist")
                .version;
            let log_version = log.version();
            prop_assert_eq!(
                reactor_version, log_version,
                "version mismatch after {} commands: reactor={} vs facts_log={}",
                n_commands, reactor_version, log_version,
            );

            Ok(())
        })?;
    }
}

// =============================================================================
// P0-10: rewind(facts, N) == FactsLog replayed up to version N
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// P0-10: PayloadUpdate sequence — rewind and FactsLog replay produce the same snapshot
    ///
    /// Differential pair:
    /// - A: rewind(&all_facts, target_version) → RewindSnapshot
    /// - B: FactsLog replay of the first target_version facts → snapshot()
    ///
    /// Two independent paths rebuild the state of the same version, cross-validating
    /// each other.
    #[test]
    fn diff_rewind_vs_factslog_payload_update(
        // 1..8 PayloadUpdate facts, each updating the "x" field
        values in proptest::collection::vec(-1000i64..1000, 1..8),
        // Target index (pick some middle version for the differential comparison)
        target_idx in 0usize..7,
    ) {
        // Generate the PayloadUpdate fact sequence
        let facts: Vec<Fact> = values.iter().enumerate().map(|(i, &v)| {
            payload_update_fact(i as u64 + 1, "x", v)
        }).collect();

        let target_idx = target_idx.min(facts.len() - 1);
        let target_facts = &facts[..=target_idx];

        // Path A: rewind
        // rewind needs the target_version; each PayloadUpdate increments the version by 1,
        // so after target_idx facts the version = target_idx + 1
        let target_version = (target_idx + 1) as u64;
        let rewind_snapshot = rewind(&facts, target_version);
        prop_assert!(
            rewind_snapshot.is_some(),
            "rewind should return Some for valid target_version={} (facts={})",
            target_version, facts.len()
        );
        let rs = rewind_snapshot.unwrap();

        // Path B: FactsLog replayed up to target_version
        let log_b = FactsLog::with_initial_payload(JsonValue::empty_object());
        for fact in target_facts {
            log_b.append(fact.clone()).expect("append failed");
        }
        let (b_payload, _b_queue, b_version) = log_b.snapshot();

        // Compare version numbers
        prop_assert_eq!(
            rs.version, b_version,
            "version mismatch: rewind={} vs facts_log={}",
            rs.version, b_version,
        );

        // Compare payloads
        let b_payload_serde = tcb_to_serde(&b_payload);
        prop_assert_eq!(
            &rs.payload, &b_payload_serde,
            "payload mismatch at version {}",
            target_version,
        );

        // Verify the x field value is correct
        let expected_x = values[target_idx];
        prop_assert_eq!(
            rs.payload.get("x").and_then(|v| v.as_i64()),
            Some(expected_x),
            "rewind x should be last update value"
        );
    }

    /// P0-10: StateTransition sequence — rewind and FactsLog replay produce the same snapshot
    ///
    /// StateTransition replaces the whole payload (rather than merging), exercising a
    /// different reconstruction path.
    #[test]
    fn diff_rewind_vs_factslog_state_transition(
        // 1..6 (x, y) pairs, each producing one StateTransition
        pairs in proptest::collection::vec(
            (-1000i64..1000, -1000i64..1000),
            1..6,
        ),
        target_idx in 0usize..5,
    ) {
        // Generate the StateTransition fact sequence
        let facts: Vec<Fact> = pairs.iter().enumerate().map(|(i, &(x, y))| {
            state_transition_fact(i as u64 + 1, i as u64, x, y)
        }).collect();

        let target_idx = target_idx.min(facts.len() - 1);
        let target_facts = &facts[..=target_idx];

        // Path A: rewind
        let target_version = (target_idx + 1) as u64;
        let rewind_snapshot = rewind(&facts, target_version);
        prop_assert!(
            rewind_snapshot.is_some(),
            "rewind should return Some for StateTransition sequence"
        );
        let rs = rewind_snapshot.unwrap();

        // Path B: FactsLog replay
        let log_b = FactsLog::with_initial_payload(JsonValue::empty_object());
        for fact in target_facts {
            log_b.append(fact.clone()).expect("append failed");
        }
        let (b_payload, _b_queue, b_version) = log_b.snapshot();

        // Compare version numbers
        prop_assert_eq!(
            rs.version, b_version,
            "version mismatch (StateTransition): rewind={} vs facts_log={}",
            rs.version, b_version,
        );

        // Compare payloads
        let b_payload_serde = tcb_to_serde(&b_payload);
        prop_assert_eq!(
            &rs.payload, &b_payload_serde,
            "payload mismatch (StateTransition) at version {}",
            target_version,
        );

        // Verify the x/y field values are correct (they should be the last
        // StateTransition's values)
        let (expected_x, expected_y) = pairs[target_idx];
        prop_assert_eq!(
            rs.payload.get("x").and_then(|v| v.as_i64()),
            Some(expected_x),
            "rewind x should match last StateTransition"
        );
        prop_assert_eq!(
            rs.payload.get("y").and_then(|v| v.as_i64()),
            Some(expected_y),
            "rewind y should match last StateTransition"
        );
    }

    /// P0-10: mixed fact sequence (PayloadUpdate + StateTransition)
    ///
    /// Verifies rewind still agrees with FactsLog on mixed fact types.
    #[test]
    fn diff_rewind_vs_factslog_mixed(
        // First one StateTransition sets the initial state, then multiple PayloadUpdates modify it
        initial_x in -500i64..500,
        initial_y in -500i64..500,
        // Each update picks either the "x" or the "y" field
        updates in proptest::collection::vec(
            (-500i64..500, proptest::sample::select(vec!["x".to_string(), "y".to_string()])),
            1..5,
        ),
        target_idx in 0usize..5,
    ) {
        let mut facts: Vec<Fact> = vec![
            state_transition_fact(1, 0, initial_x, initial_y),
        ];
        for (i, &(v, ref path)) in updates.iter().enumerate() {
            facts.push(payload_update_fact(i as u64 + 2, path, v));
        }

        let total_facts = facts.len();
        let target_idx = target_idx.min(total_facts - 1);
        let target_facts = &facts[..=target_idx];

        // target_version: 1 (StateTransition) + target_idx (PayloadUpdate count)
        let target_version = (target_idx + 1) as u64;

        // Path A: rewind
        let rewind_snapshot = rewind(&facts, target_version);
        prop_assert!(rewind_snapshot.is_some(), "rewind should return Some for mixed sequence");
        let rs = rewind_snapshot.unwrap();

        // Path B: FactsLog replay
        let log_b = FactsLog::with_initial_payload(JsonValue::empty_object());
        for fact in target_facts {
            log_b.append(fact.clone()).expect("append failed");
        }
        let (b_payload, _b_queue, b_version) = log_b.snapshot();

        // Compare version numbers
        prop_assert_eq!(
            rs.version, b_version,
            "version mismatch (mixed): rewind={} vs facts_log={}",
            rs.version, b_version,
        );

        // Compare payloads
        let b_payload_serde = tcb_to_serde(&b_payload);
        prop_assert_eq!(
            &rs.payload, &b_payload_serde,
            "payload mismatch (mixed) at version {}",
            target_version,
        );
    }
}

// =============================================================================
// P0-15: append path vs WAL replay path consistency (diff_recover_vs_append)
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// P0-15: FactsLog entry-by-entry append (in-memory WAL backend) is fully equal to
    /// replay_wal_records pure replay
    ///
    /// Differential pair:
    /// - A: FactsLog + MemoryWalStore backend, entry-by-entry append (in-process append path)
    /// - B: replay_wal_records(the WAL records produced by A) (WAL replay path, the pure
    ///   core of recover)
    ///
    /// Asserts: the snapshot / queue / version / tail-hash quadruple is fully equal, plus
    /// history length and last_stable_version. Fact-application semantics are guaranteed
    /// at the single point apply_fact_to_state on the reactor side — this differential
    /// test is the CI-resident execution face of that single-point contract.
    #[test]
    fn diff_recover_vs_append(
        specs in prop::collection::vec((0u8..5, -1000i64..1000), 0..12),
    ) {
        use evorule_reactor::{MemoryWalStore, replay_wal_records};

        let store = MemoryWalStore::new();
        let log_a = FactsLog::with_wal_store(Box::new(store.clone()));
        for (i, &(kind, seed_val)) in specs.iter().enumerate() {
            let next_id = i as u64 + 1;
            let fact = match kind % 5 {
                0 => Fact::PayloadUpdate {
                    id: FactId(next_id),
                    path: format!("p{}", next_id % 3),
                    value: JsonValue::Integer(seed_val + next_id as i64),
                },
                1 => Fact::StateTransition {
                    id: FactId(next_id),
                    cause: FactId(next_id.saturating_sub(1)),
                    new_payload: JsonValue::object_from_pairs(&[
                        ("x", JsonValue::Integer(seed_val + next_id as i64)),
                        ("y", JsonValue::Integer(next_id as i64)),
                    ]),
                    new_queue: vec![],
                },
                2 => Fact::Command {
                    id: FactId(next_id),
                    instruction: JsonValue::empty_object(),
                },
                3 => Fact::Error {
                    id: FactId(next_id),
                    message: format!("e{}", next_id),
                },
                _ => Fact::Stable {
                    id: FactId(next_id),
                    version: 0,
                },
            };
            log_a.append(fact).expect("append failed");
        }

        let (snap_a, queue_a, ver_a) = log_a.snapshot();
        let hash_a = log_a.last_hash();

        let records = store.records();
        let log_b = replay_wal_records(&records).expect("replay failed");
        let (snap_b, queue_b, ver_b) = log_b.snapshot();

        prop_assert_eq!(ver_a, ver_b, "version divergence: append={} vs replay={}", ver_a, ver_b);
        prop_assert!(
            snap_a == snap_b,
            "snapshot divergence: append={} vs replay={}",
            tcb_to_serde(&snap_a), tcb_to_serde(&snap_b),
        );
        prop_assert!(queue_a == queue_b, "queue divergence");
        let hash_b = log_b.last_hash();
        // Message kept static: left/right are moved in the prop_assert_eq expansion, and
        // the failure output automatically includes both values
        prop_assert_eq!(hash_a, hash_b, "tail-hash divergence: append vs replay");
        prop_assert_eq!(log_a.history_len(), log_b.history_len(), "history length divergence");
        prop_assert_eq!(
            log_a.last_stable_version(), log_b.last_stable_version(),
            "last_stable_version divergence",
        );
    }
}
