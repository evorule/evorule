// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `EvoRule` TCB Core - pure computation kernel
//!
//! # Design principles
//! - Zero dependencies (`#![no_std]` compatible)
//! - Pure functions (all functions are side-effect free)
//! - Strong determinism (`BTreeMap` + deterministic iteration)
//! - Never panics (path resolution returns `Option`)
//!
//! # Public interface
//! - `execute_transition()`: executes one state transition step
//! - `JsonValue`: JSON data model
//! - `TcbError`: error type
//!
//! # Quick start
//!
//! ```
//! extern crate alloc;
//! use evorule_tcb::JsonValue;
//! use alloc::collections::BTreeMap;
//!
//! // Build a JsonValue object
//! let mut map = BTreeMap::new();
//! map.insert("name".to_string(), JsonValue::string("Alice"));
//! map.insert("age".to_string(), JsonValue::Integer(30));
//! let value = JsonValue::object(map);
//!
//! // Type checks and value access
//! assert!(value.is_object());
//! let age = value.get("age").and_then(|v| v.as_i64());
//! assert_eq!(age, Some(30));
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Never-panic guarantee: the following panic-prone patterns are banned
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

extern crate alloc;

// Module declarations (in dependency order: base types first)
pub mod discipline;
pub mod domain;
pub mod envelope;
pub mod error;
pub mod executor;
pub mod path;
pub mod transition;
pub mod value;

// Core type re-exports (only stable public API)
pub use envelope::EnvelopeLimits;
pub use error::TcbError;
pub use executor::META_INSTRUCTION_TYPES;
pub use transition::{execute_transition, RuleHit, TransitionResult, MAX_TRANSFORM_RULES};
pub use value::{JsonValue, ObjectMap};
