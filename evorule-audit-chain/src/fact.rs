// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fact definition — the system's atomic communication unit

use evorule_tcb::JsonValue;

// no_std + alloc prelude (SSOT promotion: alloc-level entries of the std prelude imported explicitly)
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Unique fact identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub u64);

impl core::fmt::Display for FactId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "F{}", self.0)
    }
}

/// I/O request type (corresponds to io_type in core_eval.json)
///
/// v0.2.0: the internal representation changed from `&'static str` to
/// `Arc<str>`, letting the application layer register arbitrary io_types
/// (e.g. `retrieve`/`file`/`http`). `Arc<str>` satisfies
/// `Clone + Eq + Hash + Ord + Send + Sync`, works as a `BTreeMap`/`HashMap`
/// key, shares across threads, and clones cheaply (atomic counting).
///
/// # Impact of losing `Copy`
///
/// All by-value pass-through sites need an explicit `.clone()`. Typical case:
/// in the reactor, after `state.register_io_request(id, io_type)` the
/// `io_type` is still used in `Fact::IoRequest` and `tracing::debug!`, so it
/// needs `io_type.clone()`.
///
/// # The 5 legacy io_types
///
/// Changed from `const` to factory functions (`Arc::from` is not const). The
/// string values are unchanged, so
/// `IoType::new("call_service") == IoType::call_service()` holds; legacy WAL /
/// core_eval need no change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IoType(pub alloc::sync::Arc<str>);

impl IoType {
    /// Call an external service
    pub fn call_external() -> Self {
        Self(alloc::sync::Arc::from("call_external"))
    }
    /// Query a database
    pub fn query_db() -> Self {
        Self(alloc::sync::Arc::from("query_db"))
    }
    /// HTTP GET request
    pub fn http_get() -> Self {
        Self(alloc::sync::Arc::from("http_get"))
    }
    /// Save to memory
    pub fn save_memory() -> Self {
        Self(alloc::sync::Arc::from("save_memory"))
    }
    /// Call an external service (v0.1.x legacy path: secondary routing by `params.service_name`)
    pub fn call_service() -> Self {
        Self(alloc::sync::Arc::from("call_service"))
    }

    /// Construct an arbitrary io_type at runtime (v0.2.0 custom-IoType entry)
    ///
    /// Lets the application layer register custom io_types, e.g.
    /// `IoType::new("retrieve")`. The 5 legacy io_type string values are
    /// unchanged: `IoType::new("call_service") == IoType::call_service()`.
    pub fn new(name: &str) -> Self {
        Self(alloc::sync::Arc::from(name))
    }

    /// Parse an I/O type from a string
    ///
    /// v0.2.0: accepts unconditionally (validation moved to subscribers /
    /// `ReactorBuilder::known_io_types`). The method name is kept only for
    /// backward compatibility of old call sites; behavior changed from
    /// "return None on unknown" to "always return Some" to avoid silent breakage.
    #[deprecated(note = "since v0.2.0 use IoType::new; parse no longer validates and is kept only for backward compatibility")]
    pub fn parse(s: &str) -> Option<Self> {
        Some(Self::new(s))
    }

    /// Convert to a string
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

/// Control-flow instruction type (G8-gate compliant: single source of truth, exempted here in fact.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlFlowType {
    /// Execute multiple instructions in sequence
    Sequence,
    /// Conditional branch
    Conditional,
    /// Loop execution
    WhileLoop,
    /// Push an instruction onto the queue
    Push,
}

impl ControlFlowType {
    /// Parse a control-flow type from a string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sequence" => Some(ControlFlowType::Sequence),
            "conditional" => Some(ControlFlowType::Conditional),
            "while_loop" => Some(ControlFlowType::WhileLoop),
            "push" => Some(ControlFlowType::Push),
            _ => None,
        }
    }

    /// Convert to a string
    pub fn as_str(&self) -> &'static str {
        match self {
            ControlFlowType::Sequence => "sequence",
            ControlFlowType::Conditional => "conditional",
            ControlFlowType::WhileLoop => "while_loop",
            ControlFlowType::Push => "push",
        }
    }
}

impl core::fmt::Display for IoType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Fact ID generator (monotonically increasing)
#[derive(Debug)]
pub struct FactIdGenerator {
    next: u64,
}

impl FactIdGenerator {
    /// Create a new generator (starting from 1)
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Resume from a given value (for crash-recovery continuation)
    ///
    /// After `resume(n)`, the first `next_id()` returns `FactId(n)`.
    /// `resume(1)` equals `new()`.
    ///
    /// For crash recovery: continue from the largest FactId in the historical
    /// WAL + 1, avoiding FactId collisions between the new round and
    /// same-type Facts in the historical WAL.
    pub fn resume(from: u64) -> Self {
        Self { next: from }
    }

    /// Generate the next ID
    pub fn next_id(&mut self) -> FactId {
        let id = FactId(self.next);
        self.next += 1;
        id
    }
}

impl Default for FactIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Trace entry for domain-verdict attribution (fact-layer owned type, not bound to tcb types)
///
/// Three-state attribution booking: the result classification of
/// branch/enforce domain evaluation and the static `on_missing` declaration
/// value, carried as string labels (label values see TCB
/// `DomainOutcome::label` / `missing_reason` / `OnMissingPolicy::label`).
/// Discipline: attribution is audit-only and never fed back into execution.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceDomainAttribution {
    /// Domain-verdict outcome label: "sat" / "unsat" / "missing"
    pub outcome: String,
    /// Missing classification label: Some when outcome is "missing"
    /// ("path_not_found" / "incomparable" / "value_literal_ambiguous"),
    /// otherwise None
    pub missing_reason: Option<String>,
    /// Rule-text `on_missing` declaration value: "error" / "unsat"; None when undeclared
    pub on_missing: Option<String>,
}

impl TraceDomainAttribution {
    /// Map from a TCB domain attribution (construction-time conversion; fields remain String labels, types not bound to tcb)
    pub fn from_tcb(attr: &evorule_tcb::domain::DomainAttribution) -> Self {
        Self {
            outcome: attr.outcome.label().into(),
            missing_reason: attr.outcome.missing_reason().map(str::to_string),
            on_missing: attr.on_missing.map(str::to_string),
        }
    }
}

/// Trace entry for a single rule-hit attribution
///
/// Fact-layer owned type (not bound to tcb types, keeping crate boundaries
/// decoupled). `index` is the rule's index in the merged rule list; the source
/// label is mapped by the loading layer.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceHit {
    /// The rule's index in the merged rule list
    pub index: u64,
    /// The rule's top-level instruction type (e.g. "branch", "set")
    pub instr_type: String,
    /// Whether it structurally hit
    pub hit: bool,
    /// Domain-verdict attribution: Some only when branch/enforce instruction evaluation succeeded, None otherwise
    pub domain_attr: Option<TraceDomainAttribution>,
}

impl TraceHit {
    /// Build a fact-layer trace entry from a TCB rule-hit attribution (the single attribution-mapping exit)
    pub fn from_rule_hit(hit: &evorule_tcb::transition::RuleHit) -> Self {
        Self {
            index: hit.index as u64,
            instr_type: hit.instr_type.clone(),
            hit: hit.hit,
            domain_attr: hit
                .domain_attr
                .as_ref()
                .map(TraceDomainAttribution::from_tcb),
        }
    }
}

/// Fact — the system's atomic communication unit
///
/// All components communicate only through facts; there are no direct function calls.
///
/// # Design conventions
/// - Every variant carries `id: FactId` for global unique identification and audit tracing
/// - `StateTransition` and `IoRequest` carry `cause: FactId`, pointing to the fact that triggered them
/// - `IoResponse` carries `error: Option<String>`; None means success, Some means I/O failure
#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    /// The user submits a new instruction (triggers execution)
    Command {
        /// Unique fact identifier
        id: FactId,
        /// The instruction object to execute
        instruction: JsonValue,
    },

    /// External update of a payload field (injected by the governance layer)
    PayloadUpdate {
        /// Unique fact identifier
        id: FactId,
        /// The payload path to update
        path: String,
        /// The new value
        value: JsonValue,
    },

    /// State transition (produced automatically by the reactor)
    StateTransition {
        /// Unique fact identifier
        id: FactId,
        /// The source fact ID that triggered this transition (usually a Command or IoResponse)
        cause: FactId,
        /// The new payload after the transition
        new_payload: JsonValue,
        /// The new instruction queue after the transition
        new_queue: Vec<JsonValue>,
    },

    /// I/O request (produced by the TCB, consumed by the governance layer)
    IoRequest {
        /// Unique fact identifier, used for IoRequest ↔ IoResponse pairing
        id: FactId,
        /// The source fact ID that triggered this I/O request
        cause: FactId,
        /// The I/O type
        io_type: IoType,
        /// Request parameters
        params: JsonValue,
    },

    /// I/O response (produced by the governance layer, consumed by the reactor)
    IoResponse {
        /// Unique fact identifier
        id: FactId,
        /// The corresponding IoRequest ID
        request_id: FactId,
        /// The I/O execution result
        result: JsonValue,
        /// The I/O error message (None = success, Some = failure description)
        error: Option<String>,
    },

    /// System stable (no more executable instructions)
    Stable {
        /// Unique fact identifier
        id: FactId,
        /// The session version at stabilization
        ///
        /// # Design (O(n²) fix)
        /// The original field `final_snapshot: JsonValue` (a full payload
        /// snapshot) wrote O(n) into the fact chain per command under
        /// long-lived sessions, accumulating O(n²) (measured: ~1500 commands →
        /// 100MB WAL, 2.5s per command). The recovery path
        /// (FactsLog::recover) only updates last_stable_version for Stable and
        /// never reads the snapshot content — the snapshot was pure redundancy
        /// → slimmed down to a version number. The state body is determined by
        /// the latest StateTransition.new_payload; consumers obtain it via the
        /// snapshot API with zero information loss.
        version: u64,
    },

    /// System error (timeout or internal TCB error)
    Error {
        /// Unique fact identifier
        id: FactId,
        /// The error description
        message: String,
    },

    /// Rule-hit attribution trace (appended by the reactor after a converged transition)
    ///
    /// One entry per convergence (State/Ignored): `cause` points to the same
    /// transition's `StateTransition` or `Error`(ignored) fact; `rule_hits` is
    /// as long as the merged rule list. An `IoRequired` mid-way signal does
    /// **not** produce this fact (D11 replay contract: the post-convergence
    /// replay is authoritative, avoiding double counting).
    /// This fact does not advance the session version (a recording fact; does not change business state).
    TransitionTrace {
        /// Unique fact identifier
        id: FactId,
        /// The fact ID of the same transition's StateTransition / Error(ignored)
        cause: FactId,
        /// Per-rule hit attributions (as long as the merged rule list, in execution order)
        rule_hits: Vec<TraceHit>,
    },

    /// A violation forcibly intercepted (an `enforce` primitive hit; regression-verified)
    ///
    /// # System-exclusive (separation of powers: blocking / recording rights belong to the system)
    ///
    /// This fact is emitted **only by the reactor** upon receiving
    /// `TransitionResult::Halted` — LLMs, rules, and any instruction type have
    /// no channel to produce it (forged interception records are impossible).
    ///
    /// # Recording fact (does not bump version)
    ///
    /// Same shape as `TransitionTrace`: does not change business state. The
    /// violating instruction is **refused execution** (no StateTransition
    /// produced, not pushed back onto the queue); payload/queue stay as they
    /// were before the step.
    Violation {
        /// Unique fact identifier
        id: FactId,
        /// The source fact ID that triggered the violation (usually a Command or IoResponse)
        cause: FactId,
        /// The index of the hit enforce rule in the merged rule list
        rule_index: u64,
        /// The violation description (enforce params.reason)
        reason: String,
        /// The full echo of the rejected instruction (audit traceability of "who wanted to do what")
        instruction: JsonValue,
    },
}

impl Fact {
    /// Return the string name of the fact type (for logs)
    pub fn type_name(&self) -> &'static str {
        match self {
            Fact::Command { .. } => "Command",
            Fact::PayloadUpdate { .. } => "PayloadUpdate",
            Fact::StateTransition { .. } => "StateTransition",
            Fact::IoRequest { .. } => "IoRequest",
            Fact::IoResponse { .. } => "IoResponse",
            Fact::Stable { .. } => "Stable",
            Fact::Error { .. } => "Error",
            Fact::TransitionTrace { .. } => "TransitionTrace",
            Fact::Violation { .. } => "Violation",
        }
    }

    /// Return the fact's unique identifier
    pub fn id(&self) -> FactId {
        match self {
            Fact::Command { id, .. }
            | Fact::PayloadUpdate { id, .. }
            | Fact::StateTransition { id, .. }
            | Fact::IoRequest { id, .. }
            | Fact::IoResponse { id, .. }
            | Fact::Stable { id, .. }
            | Fact::Error { id, .. }
            | Fact::TransitionTrace { id, .. }
            | Fact::Violation { id, .. } => *id,
        }
    }

    /// Whether this is a terminal fact (Stable or Error)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Fact::Stable { .. } | Fact::Error { .. })
    }

    /// Serialize the Fact into a JsonValue (for the HTTP API to return full fact details)
    ///
    /// The returned JSON object contains the `type` field plus all fields of
    /// the variant; it can be converted to `serde_json::Value` via
    /// `tcb_to_serde` for HTTP responses.
    ///
    /// # Full-u64 losslessness (v0.9.0)
    ///
    /// FactId and other u64 fields are carried via `JsonValue::U64` (the
    /// full-u64 channel); high-bit values (e.g. the `u64::MAX-1` sentinel id of
    /// the ruleset-anchoring fact) no longer wrap around.
    /// Historical note: in the v0.8.x era this function carried high-bit FactIds
    /// via `as i64`, wrapping them negative; the then honest-boundary note
    /// (forbidden for write-side chain storage or hash inputs) was removed with
    /// the root-cause fix batch — the discipline still stands:
    /// **authoritative serialization = [`crate::hash::fact_to_stable_json`]**
    /// (the sole hash-input source, stable BTreeMap key order); this function
    /// serves the display surface only.
    //
    // Note: the match covers the serialization branch of every Fact variant,
    // deliberately kept as a single function for easy field-mapping reconciliation.
    #[allow(clippy::too_many_lines)]
    pub fn to_json(&self) -> JsonValue {
        use evorule_tcb::JsonValue as J;
        match self {
            Fact::Command { id, instruction } => J::object_from_pairs(&[
                ("type", J::string("Command")),
                ("id", J::u64(id.0)),
                ("instruction", instruction.clone()),
            ]),
            Fact::PayloadUpdate { id, path, value } => J::object_from_pairs(&[
                ("type", J::string("PayloadUpdate")),
                ("id", J::u64(id.0)),
                ("path", J::string(path.clone())),
                ("value", value.clone()),
            ]),
            Fact::StateTransition {
                id,
                cause,
                new_payload,
                new_queue,
            } => J::object_from_pairs(&[
                ("type", J::string("StateTransition")),
                ("id", J::u64(id.0)),
                ("cause", J::u64(cause.0)),
                ("new_payload", new_payload.clone()),
                ("new_queue", J::array(new_queue.clone())),
            ]),
            Fact::IoRequest {
                id,
                cause,
                io_type,
                params,
            } => J::object_from_pairs(&[
                ("type", J::string("IoRequest")),
                ("id", J::u64(id.0)),
                ("cause", J::u64(cause.0)),
                ("io_type", J::string(io_type.as_str())),
                ("params", params.clone()),
            ]),
            Fact::IoResponse {
                id,
                request_id,
                result,
                error,
            } => {
                let pairs: Vec<(&str, J)> = vec![
                    ("type", J::string("IoResponse")),
                    ("id", J::u64(id.0)),
                    ("request_id", J::u64(request_id.0)),
                    ("result", result.clone()),
                    (
                        "error",
                        match error {
                            Some(msg) => J::string(msg.clone()),
                            None => J::null(),
                        },
                    ),
                ];
                J::object_from_pairs(&pairs)
            }
            Fact::Stable { id, version } => J::object_from_pairs(&[
                ("type", J::string("Stable")),
                ("id", J::u64(id.0)),
                ("version", J::u64(*version)),
            ]),
            Fact::Error { id, message } => J::object_from_pairs(&[
                ("type", J::string("Error")),
                ("id", J::u64(id.0)),
                ("message", J::string(message.clone())),
            ]),
            Fact::TransitionTrace {
                id,
                cause,
                rule_hits,
            } => {
                // domain_attr joins the export surface — key-isomorphic with
                // trace_attr_to_json in hash.rs (outcome always present;
                // missing_reason / on_missing omitted on None). Previously the
                // export lacked this key, making offline hash recomputation ≠
                // on-chain hash for records with domain attribution. Key order
                // does not participate in hashing (the hash input is
                // stable_json's BTreeMap order); only information completeness matters here.
                let hits: Vec<J> = rule_hits
                    .iter()
                    .map(|h| {
                        let mut hit = vec![
                            ("index", J::u64(h.index)),
                            ("instr_type", J::string(h.instr_type.clone())),
                            ("hit", J::Bool(h.hit)),
                        ];
                        if let Some(attr) = &h.domain_attr {
                            let mut attr_pairs: Vec<(&str, J)> =
                                vec![("outcome", J::string(attr.outcome.clone()))];
                            if let Some(reason) = &attr.missing_reason {
                                attr_pairs.push(("missing_reason", J::string(reason.clone())));
                            }
                            if let Some(policy) = &attr.on_missing {
                                attr_pairs.push(("on_missing", J::string(policy.clone())));
                            }
                            hit.push(("domain_attr", J::object_from_pairs(&attr_pairs)));
                        }
                        J::object_from_pairs(&hit)
                    })
                    .collect();
                J::object_from_pairs(&[
                    ("type", J::string("TransitionTrace")),
                    ("id", J::u64(id.0)),
                    ("cause", J::u64(cause.0)),
                    ("rule_hits", J::array(hits)),
                ])
            }
            Fact::Violation {
                id,
                cause,
                rule_index,
                reason,
                instruction,
            } => J::object_from_pairs(&[
                ("type", J::string("Violation")),
                ("id", J::u64(id.0)),
                ("cause", J::u64(cause.0)),
                ("rule_index", J::u64(*rule_index)),
                ("reason", J::string(reason.clone())),
                ("instruction", instruction.clone()),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;

    /// Regression: the to_json export surface and the fact_to_stable_json hash
    /// surface must be information-complete and consistent — when TransitionTrace
    /// carries domain_attr (Some), the export must carry the key and be
    /// key-isomorphic. Before the fix the export dropped domain_attr → offline
    /// hash recomputation from the export ≠ the on-chain hash.
    #[test]
    fn to_json_includes_domain_attr_matching_stable_json() {
        let attr = TraceDomainAttribution {
            outcome: "missing".into(),
            missing_reason: Some("path_not_found".into()),
            on_missing: Some("unsat".into()),
        };
        let fact = Fact::TransitionTrace {
            id: FactId(9),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 0,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: Some(attr),
            }],
        };

        // Export surface: domain_attr must appear with all three keys
        let exported = fact.to_json();
        let exported_str = format!("{:?}", exported.get("rule_hits"));
        assert!(
            exported_str.contains("domain_attr"),
            "export missing domain_attr: {exported_str}"
        );
        assert!(
            exported_str.contains("path_not_found"),
            "export missing missing_reason"
        );
        assert!(exported_str.contains("unsat"), "export missing on_missing");

        // Hash surface: the same fact's stable_json also contains domain_attr — the two forms share the same information set
        let stable = crate::hash::fact_to_stable_json(&fact).unwrap();
        let stable_str = format!("{:?}", stable.get("rule_hits"));
        assert!(stable_str.contains("domain_attr"));

        // With None both forms omit it (existing legacy-shape semantics not broken)
        let fact_none = Fact::TransitionTrace {
            id: FactId(9),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 0,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: None,
            }],
        };
        let exported_none = format!("{:?}", fact_none.to_json().get("rule_hits"));
        assert!(
            !exported_none.contains("domain_attr"),
            "the export surface must also omit on None"
        );
    }
    // no_std crates link std for test builds: test macros imported explicitly
    use std::{format, vec};

    #[test]
    fn test_fact_id_generator() {
        let mut gen = FactIdGenerator::new();
        assert_eq!(gen.next_id(), FactId(1));
        assert_eq!(gen.next_id(), FactId(2));
        assert_eq!(gen.next_id(), FactId(3));
    }

    #[test]
    fn test_fact_id_generator_resume() {
        let mut gen = FactIdGenerator::resume(100);
        assert_eq!(gen.next_id(), FactId(100));
        assert_eq!(gen.next_id(), FactId(101));
        assert_eq!(gen.next_id(), FactId(102));
    }

    #[test]
    fn test_fact_id_generator_resume_from_1_equals_new() {
        let mut a = FactIdGenerator::new();
        let mut b = FactIdGenerator::resume(1);
        for _ in 0..5 {
            assert_eq!(a.next_id(), b.next_id());
        }
    }

    #[test]
    fn test_fact_type_name_and_id() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        assert_eq!(fact.type_name(), "Command");
        assert_eq!(fact.id(), FactId(1));

        let fact = Fact::Stable {
            id: FactId(2),
            version: 0,
        };
        assert_eq!(fact.type_name(), "Stable");
        assert_eq!(fact.id(), FactId(2));
        assert!(fact.is_terminal());
    }

    #[test]
    fn test_fact_cause_field() {
        let transition = Fact::StateTransition {
            id: FactId(10),
            cause: FactId(5),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };
        assert_eq!(transition.id(), FactId(10));

        let io_req = Fact::IoRequest {
            id: FactId(20),
            cause: FactId(10),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        };
        assert_eq!(io_req.id(), FactId(20));
    }

    #[test]
    fn test_io_response_with_error() {
        let ok_resp = Fact::IoResponse {
            id: FactId(1),
            request_id: FactId(2),
            result: JsonValue::string("ok"),
            error: None,
        };
        assert_eq!(ok_resp.type_name(), "IoResponse");
        assert!(!ok_resp.is_terminal());

        let err_resp = Fact::IoResponse {
            id: FactId(2),
            request_id: FactId(2),
            result: JsonValue::Null,
            error: Some("timeout".to_string()),
        };
        assert_eq!(err_resp.id(), FactId(2));
    }

    #[test]
    fn test_io_type_roundtrip() {
        // new ↔ as_str roundtrip over all IoType variants
        for expected in [
            IoType::call_external(),
            IoType::query_db(),
            IoType::http_get(),
            IoType::save_memory(),
            IoType::call_service(),
        ] {
            let s = expected.as_str();
            let constructed = IoType::new(s);
            assert_eq!(constructed, expected, "roundtrip failed for {}", s);
        }
    }

    #[test]
    fn test_io_type_parse_accepts_any() {
        // v0.2.0: parse accepts unconditionally (validation moved to subscribers / known_io_types)
        #[allow(deprecated)]
        {
            assert!(IoType::parse("unknown").is_some());
            assert!(IoType::parse("retrieve").is_some());
            assert!(IoType::parse("call_service").is_some());
            // The empty string is accepted too (validation is the upper layer's duty)
            assert!(IoType::parse("").is_some());
        }
    }

    #[test]
    fn test_io_type_new_equals_factory() {
        // v0.2.0: new and the factory functions must be equal for the same string (HashMap/BTreeMap key consistency)
        assert_eq!(IoType::new("call_service"), IoType::call_service());
        assert_eq!(IoType::new("call_external"), IoType::call_external());
        assert_eq!(IoType::new("retrieve"), IoType::new("retrieve"));
        assert_ne!(IoType::new("retrieve"), IoType::new("file"));
    }

    #[test]
    fn test_io_type_display() {
        assert_eq!(format!("{}", IoType::call_external()), "call_external");
        assert_eq!(format!("{}", IoType::query_db()), "query_db");
        assert_eq!(format!("{}", IoType::http_get()), "http_get");
        assert_eq!(format!("{}", IoType::save_memory()), "save_memory");
        assert_eq!(format!("{}", IoType::call_service()), "call_service");
    }

    #[test]
    fn test_fact_id_display() {
        assert_eq!(format!("{}", FactId(0)), "F0");
        assert_eq!(format!("{}", FactId(1)), "F1");
        assert_eq!(format!("{}", FactId(42)), "F42");
    }

    #[test]
    fn test_violation_fact_to_json_and_meta() {
        // Regression: Violation fact serialization + meta info (type_name/id/is_terminal)
        let fact = Fact::Violation {
            id: FactId(9),
            cause: FactId(3),
            rule_index: 2,
            reason: "violation: deleting the dataset is forbidden".to_string(),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("delete_all"))]),
        };

        assert_eq!(fact.type_name(), "Violation");
        assert_eq!(fact.id(), FactId(9));
        // Recording fact: non-terminal (does not end the session; the session keeps processing the queue)
        assert!(!fact.is_terminal());

        let json = fact.to_json();
        assert_eq!(json.get("type"), Some(&JsonValue::string("Violation")));
        // Since v0.9.0 u64 fields go through the full-u64 U64 channel (root fix for the as i64 wraparound)
        assert_eq!(json.get("id"), Some(&JsonValue::U64(9)));
        assert_eq!(json.get("cause"), Some(&JsonValue::U64(3)));
        assert_eq!(json.get("rule_index"), Some(&JsonValue::U64(2)));
        assert_eq!(
            json.get("reason"),
            Some(&JsonValue::string("violation: deleting the dataset is forbidden"))
        );
        assert_eq!(
            json.get("instruction").and_then(|v| v.get("type")),
            Some(&JsonValue::string("delete_all"))
        );
    }

    /// u64 wraparound root-fix regression (v0.9.0): high-bit FactIds carried
    /// losslessly through to_json — the u64::MAX-1 sentinel id of the
    /// ruleset-anchoring fact no longer wraps negative.
    #[test]
    fn test_to_json_sentinel_fact_id_lossless() {
        let sentinel = u64::MAX - 1;
        let fact = Fact::PayloadUpdate {
            id: FactId(sentinel),
            path: "__ruleset__.version".into(),
            value: JsonValue::u64(7),
        };
        let json = fact.to_json();
        assert_eq!(json.get("id").and_then(|v| v.as_u64()), Some(sentinel));
        // Legacy channel form check: id is U64 not Integer (the i64-wraparound channel is retired)
        assert!(json.get("id").is_some_and(|v| v.is_u64()));
        // Roundtrip: the hash surface (serde Number lossless u64) and the display surface agree on the same sentinel
        let stable = crate::hash::fact_to_stable_json(&fact).unwrap();
        assert_eq!(stable.get("id").and_then(|v| v.as_u64()), Some(sentinel));
    }

    #[test]
    fn test_fact_is_terminal_all_variants() {
        // Terminal facts
        assert!(Fact::Stable {
            id: FactId(1),
            version: 0,
        }
        .is_terminal());
        assert!(Fact::Error {
            id: FactId(2),
            message: "err".to_string(),
        }
        .is_terminal());

        // Non-terminal facts
        assert!(!Fact::Command {
            id: FactId(3),
            instruction: JsonValue::empty_object(),
        }
        .is_terminal());
        assert!(!Fact::PayloadUpdate {
            id: FactId(4),
            path: "x".to_string(),
            value: JsonValue::Null,
        }
        .is_terminal());
        assert!(!Fact::StateTransition {
            id: FactId(5),
            cause: FactId(0),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        }
        .is_terminal());
        assert!(!Fact::IoRequest {
            id: FactId(6),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        }
        .is_terminal());
        assert!(!Fact::IoResponse {
            id: FactId(7),
            request_id: FactId(6),
            result: JsonValue::Null,
            error: None,
        }
        .is_terminal());
    }

    #[test]
    fn test_fact_type_name_all_variants() {
        assert_eq!(
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            }
            .type_name(),
            "Command"
        );
        assert_eq!(
            Fact::PayloadUpdate {
                id: FactId(1),
                path: "x".to_string(),
                value: JsonValue::Null,
            }
            .type_name(),
            "PayloadUpdate"
        );
        assert_eq!(
            Fact::StateTransition {
                id: FactId(1),
                cause: FactId(0),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            }
            .type_name(),
            "StateTransition"
        );
        assert_eq!(
            Fact::IoRequest {
                id: FactId(1),
                cause: FactId(0),
                io_type: IoType::call_external(),
                params: JsonValue::empty_object(),
            }
            .type_name(),
            "IoRequest"
        );
        assert_eq!(
            Fact::IoResponse {
                id: FactId(1),
                request_id: FactId(0),
                result: JsonValue::Null,
                error: None,
            }
            .type_name(),
            "IoResponse"
        );
        assert_eq!(
            Fact::Stable {
                id: FactId(1),
                version: 0,
            }
            .type_name(),
            "Stable"
        );
        assert_eq!(
            Fact::Error {
                id: FactId(1),
                message: "e".to_string(),
            }
            .type_name(),
            "Error"
        );
    }

    #[test]
    fn test_fact_id_generator_default() {
        // Default starts from 0; next_id starts from 1
        let mut gen = FactIdGenerator::default();
        assert_eq!(gen.next_id(), FactId(1));
    }
}
