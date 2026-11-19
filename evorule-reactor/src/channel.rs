// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Dual-channel wrapper (command mpsc + event broadcast)
//!
//! # Design basis
//! Based on the `02 Reactive Data Executor` design doc §3.3 and the
//! `04 Tree Structure` doc's channel.rs definition:
//! - **command channel**: `mpsc::unbounded_channel`, single receiver (the reactor), guaranteeing FIFO
//! - **event channel**: `broadcast::channel`, multiple receivers (user / governance-layer I/O subscribers / auditor)
//!
//! # Dual-channel architecture
//! - **command channel**: user/governance → reactor (submits Command/PayloadUpdate/IoResponse)
//! - **event channel**: reactor → user/governance (produces StateTransition/IoRequest/Stable/Error)
//!   - broadcast lets evorule-governance's I/O subscribers and the auditor subscribe simultaneously

use crate::fact::Fact;
use tokio::sync::{broadcast, mpsc};

/// Command-channel sender (cloneable, for sharing across multiple components)
pub type FactSender = mpsc::UnboundedSender<Fact>;

/// Command-channel receiver (unique, held by the reactor)
pub type FactReceiver = mpsc::UnboundedReceiver<Fact>;

/// Event-channel sender (cloneable, held by the reactor; `subscribe()` creates new receivers)
pub type EventSender = broadcast::Sender<Fact>;

/// Event-channel receiver (cloneable; held individually by user / governance-layer I/O subscribers / auditor)
pub type EventReceiver = broadcast::Receiver<Fact>;

/// Broadcast channel capacity
///
/// Facts produced by the reactor are not very frequent; 1024 is enough to hold a
/// full Command → StateTransition → IoRequest → IoResponse → Stable round.
/// If a receiver falls behind by more than this capacity, it receives
/// `RecvError::Lagged`.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Channel pair (command mpsc + event broadcast)
///
/// Wraps the creation logic of both channels, ensuring type consistency.
pub struct ChannelPair {
    /// Command-channel sender (held by the user)
    pub command_tx: FactSender,
    /// Command-channel receiver (held by the reactor)
    pub command_rx: FactReceiver,
    /// Event-channel sender (held by the reactor)
    pub event_tx: EventSender,
    /// Event-channel receiver (the first subscriber, held by the user)
    pub event_rx: EventReceiver,
}

impl ChannelPair {
    /// Creates a new channel pair
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            command_tx,
            command_rx,
            event_tx,
            event_rx,
        }
    }
}

impl Default for ChannelPair {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::FactId;
    use evorule_tcb::JsonValue;

    #[tokio::test]
    async fn test_command_channel_fifo_order() {
        let mut pair = ChannelPair::new();

        // Send three facts to the command channel
        pair.command_tx
            .send(Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        pair.command_tx
            .send(Fact::Command {
                id: FactId(2),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        pair.command_tx
            .send(Fact::Command {
                id: FactId(3),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();

        // Verify FIFO order
        let f1 = pair.command_rx.recv().await.unwrap();
        let f2 = pair.command_rx.recv().await.unwrap();
        let f3 = pair.command_rx.recv().await.unwrap();

        assert_eq!(f1.id(), FactId(1));
        assert_eq!(f2.id(), FactId(2));
        assert_eq!(f3.id(), FactId(3));
    }

    #[test]
    fn test_command_sender_clone() {
        let pair = ChannelPair::new();
        let tx_clone = pair.command_tx.clone();

        // The cloned sender should be able to send
        tx_clone
            .send(Fact::Stable {
                id: FactId(1),
                version: 0,
            })
            .unwrap();

        // The original sender can also send
        pair.command_tx
            .send(Fact::Stable {
                id: FactId(2),
                version: 0,
            })
            .unwrap();
    }

    #[tokio::test]
    async fn test_event_broadcast_multiple_subscribers() {
        let mut pair = ChannelPair::new();

        // Create a second subscriber
        let mut rx2 = pair.event_tx.subscribe();

        // Send to the event channel
        pair.event_tx
            .send(Fact::Stable {
                id: FactId(1),
                version: 0,
            })
            .unwrap();

        // Both receivers should receive it
        let f1 = pair.event_rx.recv().await.unwrap();
        let f2 = rx2.recv().await.unwrap();
        assert_eq!(f1.id(), FactId(1));
        assert_eq!(f2.id(), FactId(1));
    }
}
