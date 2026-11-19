// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O Handler trait — object-safe, supports dynamic dispatch via trait objects
//!
//! # H5 migration background
//!
//! This trait originally lived in `evorule-governance/src/io_handler.rs` with an
//! `impl Future` (RPITIT) signature, which is not object-safe and cannot be used
//! as `dyn IoHandler`.
//!
//! During the H5 architecture-debt fix, the trait was sunk down into
//! evorule-reactor (same layer as `IoType`) and switched to `#[async_trait]`,
//! making it object-safe. This way:
//! - Application-layer crates (`evorule-io-handlers`) can depend on
//!   evorule-reactor (rather than evorule-governance), avoiding circular dependencies
//! - `IoDispatcher` can hold `HashMap<IoType, Arc<dyn IoHandler>>` and register
//!   handlers dynamically
//!
//! # Usage example
//!
//! ```
//! use async_trait::async_trait;
//! use evorule_reactor::{IoHandler, IoResult};
//! use evorule_tcb::JsonValue;
//!
//! struct EchoHandler;
//!
//! #[async_trait]
//! impl IoHandler for EchoHandler {
//!     async fn execute(&self, params: &JsonValue) -> IoResult {
//!         Ok(params.clone())
//!     }
//! }
//! ```

use async_trait::async_trait;
use evorule_tcb::JsonValue;

/// I/O execution result
///
/// On success returns a `JsonValue` (injected into `payload.__io_results__.{io_type}`);
/// on failure returns an error message (recorded in the `IoResponse.error` field).
pub type IoResult = Result<JsonValue, String>;

/// I/O Handler trait — all I/O handlers implement this interface
///
/// Uses `#[async_trait]` to make the trait object-safe, supporting `Arc<dyn IoHandler>`.
///
/// `params` comes from `Fact::IoRequest.params`, already resolved by the TCB from
/// path references into concrete values.
///
/// # Implementer responsibilities
/// - Extract the required fields from `params` (e.g. `url`, `query`, `key`)
/// - Perform the actual I/O operation (HTTP requests, SQL queries, file reads/writes, etc.)
/// - Return `Ok(JsonValue)` or `Err(String)`
///
/// # Callers
/// - `evorule-governance::IoDispatcher` dispatches to the corresponding handler by `IoType`
/// - `evorule-governance::IoSubscriber` subscribes to `Fact::IoRequest` and calls dispatch
#[async_trait]
pub trait IoHandler: Send + Sync {
    /// Executes the I/O operation
    ///
    /// # Arguments
    /// - `params`: I/O request parameters (passed through by the `io_request` instruction; e.g.
    ///   `call_external` takes `{"messages": [...], "tools": [...]}`,
    ///   `call_service` takes `{"service_name": "...", "args": {...}}`)
    ///
    /// # Returns
    /// - `Ok(JsonValue)`: the I/O result (injected into `payload.__io_results__.{io_type}`)
    /// - `Err(String)`: error message (recorded in `IoResponse.error`; the result is not consumed)
    async fn execute(&self, params: &JsonValue) -> IoResult;
}
