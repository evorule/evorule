// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O Dispatcher — dynamic dispatch via trait objects (mechanism layer; sunk from
//! governance to reactor in v0.2.0)
//!
//! # v0.2.0 sinking background
//!
//! In v0.1.x `IoDispatcher` lived in evorule-governance, but the agent (solution 1)
//! did not depend on governance, so the agent could not reuse IoDispatcher and had
//! to detour via `call_service` + secondary routing. In v0.2.0 the custom-IoType
//! capability requires the agent to register handlers directly by IoType, so
//! IoDispatcher was sunk down into evorule-reactor (the mechanism-layer base);
//! both governance and the agent can reuse it.
//!
//! # Design
//!
//! `HashMap<IoType, Arc<dyn IoHandler>>` trait-object pattern:
//! - Concrete handler implementations are injected by the application layer
//! - Registered dynamically via `builder()` + `register()`
//! - The core layer is unaware of concrete implementation types (decoupled from sqlx/reqwest etc.)
//!
//! # Performance
//!
//! Trait-object dynamic dispatch is imperceptible for I/O operations: after a
//! dispatch call it immediately awaits, and the nanosecond-scale vtable lookup
//! overhead is negligible compared to millisecond-scale I/O latency.
//!
//! `IoType` as a `HashMap` key: since v0.2.0 `IoType(pub Arc<str>)` implements
//! `Hash + Eq`; the hash is based on the `str` content, so `IoType` values with the
//! same string (whether built via `new` or a factory function) hash identically.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{IoHandler, IoResult, IoType};
use evorule_tcb::JsonValue;

/// I/O dispatcher (mechanism layer)
///
/// Holds `HashMap<IoType, Arc<dyn IoHandler>>` and dispatches to the corresponding
/// handler by IoType. Concrete handlers are registered by the application layer;
/// the core layer is unaware of concrete implementation types.
///
/// `Clone`: the internal handlers are all `Arc<dyn IoHandler>`; cloning only bumps
/// reference counts and never copies the handlers themselves. In multi-session mode
/// each session can share the same set of handlers.
#[derive(Clone)]
pub struct IoDispatcher {
    /// IoType → handler map
    handlers: HashMap<IoType, Arc<dyn IoHandler>>,
}

impl IoDispatcher {
    /// Creates an empty dispatcher (no handlers registered)
    ///
    /// Usually handlers are registered in a chain via `IoDispatcher::builder()`.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Registers a handler for the given IoType
    ///
    /// If the IoType already has a handler, the old handler is overwritten.
    pub fn register(&mut self, io_type: IoType, handler: Arc<dyn IoHandler>) {
        self.handlers.insert(io_type, handler);
    }

    /// Returns a builder for chaining handler registrations
    ///
    /// # Example
    /// ```ignore
    /// let dispatcher = IoDispatcher::builder()
    ///     .register(IoType::call_external(), Arc::new(llm_handler))
    ///     .register(IoType::call_service(), Arc::new(service_handler))
    ///     .build();
    /// ```
    pub fn builder() -> IoDispatcherBuilder {
        IoDispatcherBuilder::new()
    }

    /// Dispatches execution by IoType
    ///
    /// If no handler is registered for the IoType, returns an error (so the reactor
    /// becomes aware of it, rather than failing silently).
    pub async fn dispatch(&self, io_type: &IoType, params: &JsonValue) -> IoResult {
        match self.handlers.get(io_type) {
            Some(handler) => handler.execute(params).await,
            None => {
                tracing::warn!(
                    "Unknown IoType: {}, no handler registered",
                    io_type.as_str()
                );
                Err(format!(
                    "no handler registered for IoType: {}",
                    io_type.as_str()
                ))
            }
        }
    }

    /// Whether a handler exists for the given IoType
    ///
    /// Used by load-time validation (Capability 7, `validate_core_eval_io_types`).
    pub fn contains(&self, io_type: &IoType) -> bool {
        self.handlers.contains_key(io_type)
    }

    /// All registered IoTypes
    ///
    /// Used by `ReactorBuilder::known_io_types` fail-fast validation (Capability 4):
    /// a reactor with a registered dispatcher can reject io_types outside the set
    /// (a typo immediately yields a `Fact::Error`, rather than being passed through
    /// to the subscriber).
    pub fn known_types(&self) -> impl Iterator<Item = &IoType> {
        self.handlers.keys()
    }
}

impl Default for IoDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// I/O dispatcher builder (chained registration)
///
/// Created via `IoDispatcher::builder()`; chain `register()` calls, then finish
/// with `build()`.
pub struct IoDispatcherBuilder {
    handlers: HashMap<IoType, Arc<dyn IoHandler>>,
}

impl IoDispatcherBuilder {
    /// Creates an empty builder
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Registers a handler (chained call)
    pub fn register(mut self, io_type: IoType, handler: Arc<dyn IoHandler>) -> Self {
        self.handlers.insert(io_type, handler);
        self
    }

    /// Builds the IoDispatcher
    pub fn build(self) -> IoDispatcher {
        IoDispatcher {
            handlers: self.handlers,
        }
    }
}

impl Default for IoDispatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoHandler;
    #[async_trait::async_trait]
    impl IoHandler for EchoHandler {
        async fn execute(&self, params: &JsonValue) -> IoResult {
            Ok(params.clone())
        }
    }

    #[test]
    fn dispatcher_routes_by_io_type() {
        let mut d = IoDispatcher::new();
        d.register(IoType::new("retrieve"), Arc::new(EchoHandler));
        assert!(d.contains(&IoType::new("retrieve")));
        assert!(!d.contains(&IoType::new("file")));
        assert_eq!(d.known_types().count(), 1);
    }

    #[tokio::test]
    async fn dispatch_hit_and_miss() {
        let d = IoDispatcher::builder()
            .register(IoType::new("retrieve"), Arc::new(EchoHandler))
            .build();
        let params = JsonValue::Null;
        assert!(d.dispatch(&IoType::new("retrieve"), &params).await.is_ok());
        assert!(d.dispatch(&IoType::new("file"), &params).await.is_err());
    }

    #[test]
    fn new_equals_factory_key_collision() {
        // IoType::new("call_service") and IoType::call_service() hash identically — the same key
        let mut d = IoDispatcher::new();
        d.register(IoType::call_service(), Arc::new(EchoHandler));
        assert!(d.contains(&IoType::new("call_service")));
    }
}
