// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EvoRule governance layer - I/O subscribers, audit chain, rule validation, time machine
//!
//! # Positioning
//! evorule-governance is the top layer of the three-layer architecture (a pure
//! mechanism-layer library):
//! - subscribes to the event broadcast channel of evorule-reactor and filters `IoRequest` facts
//! - dispatches I/O through the IoDispatcher framework (concrete Handler implementations are
//!   injected by the application layer; this crate only defines the mechanism)
//! - builds the audit chain on top of `FactsLog` (BLAKE3 hashing + logical clock)
//! - provides the rule validator (RuleValidator) + time machine (TimeMachine: replay / rewind / fork / diff)
//! - provides SessionManager: a session manager (mechanism layer, manages reactor instance lifecycles)
//!
//! **No longer included after the H5/H6 boundary cleanup**: HTTP API, SSE, Prometheus metrics,
//! Bearer authentication, concrete I/O Handler implementations.
//! The above application-layer features are built by the application layer itself; this crate
//! only provides mechanism-layer interfaces.
//!
//! # Design principles
//! - **Never touches control flow**: the governance layer is completely unaware of the existence
//!   of `conditional`/`while_loop`/`sequence`
//! - **I/O as an add-on**: only defines the IoHandler trait + IoDispatcher framework (mechanism),
//!   with zero business-logic intrusion; concrete implementations are injected by the application layer
//! - **Fact bus**: all components communicate through Fact, with no direct function calls
//!
//! # Module structure
//! - `io_handler` — IoHandler trait + IoResult (mechanism definition; implementations injected by the application layer)
//! - `io_dispatcher` — Enum Dispatch (I/O dispatch framework)
//! - `io_subscriber` — subscribes IoRequest → dispatch → write back IoResponse
//! - `auditor` — audit chain built on FactsLog
//! - `hash` — BLAKE3 hashing
//! - `clock` — logical clock
//! - `session` — session manager (mechanism layer, manages reactor instance lifecycles)
//! - `rule_validation` — rule JSON Schema validator (tier0 core_eval.json)
//! - `time_machine` — time machine: replay / rewind / fork / diff
//! - `shared_facts_log` — cross-session shared FactsLog wrapper
//! - `metrics` — IoMetrics trait (mechanism-layer interface; the Prometheus implementation is provided by the application layer)
// cluster removed (multi-reactor collaboration primitives, an application-layer feature to be implemented by the application layer later)
// api, io_handlers and bin directories have all been moved out of this crate (H5/H6 boundary cleanup)

#![forbid(unsafe_code)]
#![deny(missing_docs)]

// H6: the api module has been moved out of this crate (mechanism-policy separation; the core layer no longer contains HTTP API code)
pub mod anchor;
pub mod auditor;
pub mod circuit_breaker;
pub mod clock;
pub mod hash;
pub mod io_dispatcher;
pub mod io_handler;
// H5: concrete io_handlers implementations have been moved out (mechanism-policy separation)
// concrete handler implementations (DbHandler/HttpHandler/MemoryHandler) no longer belong to this crate
pub mod io_subscriber;
pub mod metrics;
pub mod permission;
// object_pool removed (performance optimization, non-core feature)
pub mod rule_validation;
// H6: the session module has been promoted from api/ to the top level (mechanism layer, manages reactor instance lifecycles)
pub mod session;
// G-A1: audit anchor signing (authenticity/non-repudiation), an optional capability — when no signer is configured, audit behavior is identical to the previous version
pub mod shared_facts_log;
pub mod signing;
pub mod time_machine;

// Public API re-exports
// H6: AppState/GovernanceApi/GovernanceServer/SessionApi have moved to the application layer (no longer exported from core)
pub use auditor::{Auditor, LoadError};
pub use clock::LogicalClock;
// cluster removed (multi-reactor collaboration primitives, an application-layer feature)
pub use hash::content_hash;
pub use io_dispatcher::{IoDispatcher, IoDispatcherBuilder};
pub use io_handler::{IoHandler, IoResult};
pub use io_subscriber::IoSubscriber;
pub use metrics::{noop_metrics, IoMetrics, NoOpMetrics, SharedMetrics};
pub use permission::{
    ConditionEvaluator, DefaultPolicy, PermissionEntry, PermissionError, PermissionGate,
    PermissionState, PermissionTable, Verdict,
};
pub use session::{Session, SessionAnchorConfig, SessionError, SessionId, SessionManager};
// G-A1: audit anchor signing (authenticity/non-repudiation)
pub use anchor::{
    signature_payload_v2, verify_anchor_chain, verify_binding, AnchorChainBuilder, AnchorError,
    AnchorProto, AuditAnchor, FactRange, ANCHOR_FORMAT, ANCHOR_GENESIS,
};
pub use signing::{verify_signature, AuditSigner, SignError};
// ObjectPool removed (performance optimization, non-core feature)
pub use shared_facts_log::{SharedFact, SharedFactsLog};
