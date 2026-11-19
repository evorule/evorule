// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Single-threaded local queues (the concretization of single-threaded channels;
//! groundwork for the embedded synchronous executor)
//!
//! # Positioning (honest statement)
//!
//! [`crate::channel`]'s tokio `mpsc/broadcast` relies on `Send + Sync` bounds and
//! a multi-threaded executor; single-threaded hosts (the embedded synchronous
//! executor, browser workers) do not need — and cannot afford — those bounds.
//! This module provides a single-threaded implementation with the same **method
//! surface**:
//!
//! - **No tokio, no locks**: `Rc<RefCell<VecDeque>>` purely single-threaded data structures
//! - **Type-level single-thread enforcement**: `Rc` makes every type
//!   `!Send + !Sync` — cross-thread use is invisible at compile time, not a runtime convention
//! - **Verbatim-identical FIFO**: command semantics (unbounded, single receiver)
//!   and event semantics (multiple subscribers, new subscribers receive no history)
//!   align with the tokio versions
//!
//! # Boundary (honest differences from the tokio versions)
//!
//! - **Synchronous surface**: this module only provides `send/try_recv`; async
//!   blocking `recv().await` is the driving responsibility of the synchronous
//!   executor (a later batch) — this module does not pre-build an async layer
//! - **No capacity cap / no Lagged**: event subscription queues are unbounded;
//!   tokio broadcast's capacity-Lagged semantics is not replicated (embedded host
//!   memory is bounded; overflow should fail explicitly rather than silently drop Facts)
//! - **Not wired into the production path in this batch**: the native production
//!   channels remain the tokio versions (byte-equivalence iron rule); the switch
//!   happens in the synchronous-executor landing batch
//!
//! # Usage
//!
//! ```ignore
//! use evorule_reactor::channel_local::{local_pair, LocalBroadcaster};
//!
//! let (tx, rx) = local_pair::<Fact>();        // command surface (mpsc semantics)
//! let (evt_tx, evt_rx) = local_broadcaster::<Fact>(); // event surface (broadcast semantics)
//! tx.push(fact)?;
//! let fact = rx.try_pop().unwrap();
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

/// Command-surface error (a minimal semantic surface aligned with `mpsc::SendError`)
#[derive(Debug, PartialEq, Eq)]
pub struct LocalSendError;

impl std::fmt::Display for LocalSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "local queue receiver dropped")
    }
}

impl std::error::Error for LocalSendError {}

/// Shared queue core (sender and receiver each hold a strong/weak reference pair)
struct SharedQueue<T> {
    deque: RefCell<VecDeque<T>>,
}

/// Single-threaded command sender (unbounded mpsc semantics; cloneable, sharing the same queue)
///
/// Holds a weak reference: once the receiver is dropped, `send` fails
/// (aligned with `mpsc::SendError` semantics).
pub struct LocalSender<T> {
    inner: Weak<SharedQueue<T>>,
}

impl<T> Clone for LocalSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> LocalSender<T> {
    /// Enqueues (unbounded: never fails due to capacity; only fails after the receiver is dropped)
    pub fn send(&self, value: T) -> Result<(), LocalSendError> {
        self.inner
            .upgrade()
            .ok_or(LocalSendError)?
            .deque
            .borrow_mut()
            .push_back(value);
        Ok(())
    }

    /// Current backlog length (`None` after the receiver is dropped)
    pub fn len(&self) -> Option<usize> {
        Some(self.inner.upgrade()?.deque.borrow().len())
    }

    /// Whether the queue is empty (`None` after the receiver is dropped)
    pub fn is_empty(&self) -> Option<bool> {
        Some(self.inner.upgrade()?.deque.borrow().is_empty())
    }
}

impl<T> std::fmt::Debug for LocalSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSender").finish_non_exhaustive()
    }
}

/// Single-threaded command receiver (single-consumer FIFO)
///
/// Holds a strong reference: the queue is owned by the receiver — this is the
/// anchor of the `send` failure criterion.
pub struct LocalReceiver<T> {
    inner: Rc<SharedQueue<T>>,
}

impl<T> LocalReceiver<T> {
    /// Dequeues: returns `None` when the queue is empty
    pub fn try_pop(&self) -> Option<T> {
        self.inner.deque.borrow_mut().pop_front()
    }

    /// Current backlog length
    pub fn len(&self) -> usize {
        self.inner.deque.borrow().len()
    }

    /// Whether the queue is empty
    pub fn is_empty(&self) -> bool {
        self.inner.deque.borrow().is_empty()
    }
}

impl<T> std::fmt::Debug for LocalReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalReceiver").finish_non_exhaustive()
    }
}

/// Creates a single-threaded command channel pair (unbounded mpsc semantics)
pub fn local_pair<T>() -> (LocalSender<T>, LocalReceiver<T>) {
    let inner = Rc::new(SharedQueue {
        deque: RefCell::new(VecDeque::new()),
    });
    (
        LocalSender {
            inner: Rc::downgrade(&inner),
        },
        LocalReceiver { inner },
    )
}

/// Event broadcaster core (subscriber list; the sender holds strong references,
/// each subscriber holds its own queue)
struct BroadcasterCore<T> {
    subscribers: RefCell<Vec<Weak<RefCell<VecDeque<T>>>>>,
}

/// Single-threaded event sender (broadcast semantics: multiple subscribers, each
/// with an independent queue, new subscribers receive no history)
pub struct LocalEventSender<T> {
    core: Rc<BroadcasterCore<T>>,
}

impl<T> Clone for LocalEventSender<T> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
        }
    }
}

impl<T: Clone> LocalEventSender<T> {
    /// Broadcast: delivers one clone to every live subscriber
    ///
    /// Discarded subscribers (receivers already dropped) are reclaimed here,
    /// aligned with tokio broadcast's dead-subscriber cleanup semantics.
    pub fn send(&self, value: &T) {
        let mut subs = self.core.subscribers.borrow_mut();
        let mut dead = Vec::new();
        for (idx, weak) in subs.iter().enumerate() {
            match weak.upgrade() {
                Some(q) => q.borrow_mut().push_back(value.clone()),
                None => dead.push(idx),
            }
        }
        for idx in dead.into_iter().rev() {
            subs.swap_remove(idx);
        }
    }

    /// Current number of live subscribers
    pub fn subscriber_count(&self) -> usize {
        self.core.subscribers.borrow().len()
    }
}

impl<T> std::fmt::Debug for LocalEventSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEventSender").finish_non_exhaustive()
    }
}

/// Single-threaded event receiver (independent queue per subscriber)
pub struct LocalEventReceiver<T> {
    queue: Rc<RefCell<VecDeque<T>>>,
}

impl<T> LocalEventReceiver<T> {
    /// Dequeues: returns `None` when the queue is empty
    pub fn try_pop(&self) -> Option<T> {
        self.queue.borrow_mut().pop_front()
    }

    /// Current backlog length
    pub fn len(&self) -> usize {
        self.queue.borrow().len()
    }

    /// Whether the queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }
}

impl<T> Clone for LocalEventReceiver<T> {
    /// Cloning shares the same subscription queue (the multiple-consumer form of
    /// one subscriber); new subscribers should be created via
    /// [`LocalEventSender::subscribe`].
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
        }
    }
}

impl<T> std::fmt::Debug for LocalEventReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEventReceiver").finish_non_exhaustive()
    }
}

/// Creates a single-threaded event broadcast channel (broadcast semantics: new subscribers receive no history)
///
/// Returns (sender, first-subscriber receiver) — aligned with the event surface of
/// [`crate::channel::ChannelPair`]; later subscribers are created via
/// [`LocalEventSender::subscribe`].
pub fn local_broadcaster<T>() -> (LocalEventSender<T>, LocalEventReceiver<T>) {
    let core = Rc::new(BroadcasterCore {
        subscribers: RefCell::new(Vec::new()),
    });
    let first = Rc::new(RefCell::new(VecDeque::new()));
    core.subscribers.borrow_mut().push(Rc::downgrade(&first));
    (
        LocalEventSender { core },
        LocalEventReceiver { queue: first },
    )
}

impl<T> LocalEventSender<T> {
    /// Creates a new subscriber (receives no history — aligned with tokio broadcast semantics)
    pub fn subscribe(&self) -> LocalEventReceiver<T> {
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        self.core
            .subscribers
            .borrow_mut()
            .push(Rc::downgrade(&queue));
        LocalEventReceiver { queue }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic)]
    use super::*;

    /// Command surface FIFO: first in first out, unbounded, cloned senders share the queue
    #[test]
    fn local_pair_fifo_and_shared_sender() {
        let (tx, rx) = local_pair::<u64>();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        let tx2 = tx.clone();
        tx2.send(3).unwrap();
        assert_eq!(rx.len(), 3);
        assert_eq!(rx.try_pop(), Some(1));
        assert_eq!(rx.try_pop(), Some(2));
        assert_eq!(rx.try_pop(), Some(3));
        assert!(rx.try_pop().is_none());
        assert_eq!(rx.len(), 0);
        assert_eq!(tx.is_empty(), Some(true));
    }

    /// After the receiver is dropped, sender.send fails (LocalSendError, aligned with mpsc semantics)
    #[test]
    fn local_pair_receiver_drop_yields_send_error() {
        let (tx, rx) = local_pair::<u64>();
        drop(rx);
        assert_eq!(tx.send(1), Err(LocalSendError));
        assert_eq!(tx.len(), None);
    }

    /// Broadcast surface: each subscriber gets its own copy, new subscribers receive no
    /// history, dead subscribers are reclaimed
    #[test]
    fn broadcaster_fanout_no_history_and_dead_cleanup() {
        let (tx, rx1) = local_broadcaster::<u64>();
        tx.send(&10);
        let rx2 = tx.subscribe(); // receives no history
        assert!(rx2.try_pop().is_none());
        tx.send(&20);
        assert_eq!(rx1.try_pop(), Some(10));
        assert_eq!(rx1.try_pop(), Some(20));
        assert_eq!(rx2.try_pop(), Some(20));
        assert_eq!(tx.subscriber_count(), 2);
        drop(rx1);
        tx.send(&30); // triggers dead-subscriber reclamation
        assert_eq!(tx.subscriber_count(), 1);
        assert_eq!(rx2.try_pop(), Some(30));
    }

    /// Type-level single-thread enforcement: Rc-based types must be !Send
    /// (runtime mirror of the compile-time guarantee)
    #[test]
    fn local_types_are_not_send() {
        fn assert_not_send<T: 'static>() {}
        assert_not_send::<LocalSender<u8>>();
        assert_not_send::<LocalReceiver<u8>>();
        assert_not_send::<LocalEventSender<u8>>();
        assert_not_send::<LocalEventReceiver<u8>>();
    }

    /// Sender-side len semantics: receiver alive → Some; receiver dropped → None
    #[test]
    fn receiver_len_tracks_sender_liveness() {
        let (tx, rx) = local_pair::<u64>();
        tx.send(1).unwrap();
        assert_eq!(rx.len(), 1);
        assert_eq!(tx.len(), Some(1));
        drop(rx);
        assert_eq!(tx.len(), None);
        assert_eq!(tx.send(2), Err(LocalSendError));
    }
}
