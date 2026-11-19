// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor internal state

use crate::fact::{FactId, IoType};
use crate::phase::ReactorPhase;
use evorule_tcb::JsonValue;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

// ============================================================================
// Kani-specific collection types
//
// Kani has limited ability to model the red-black tree internals of
// BTreeSet/BTreeMap (see evorule-tcb/verification/kani-formal-verification-design.md).
// Even with only 1 element in the set, insert/remove operations unroll a large
// number of red-black tree node operation paths, causing CBMC state explosion.
//
// Solution: under #[cfg(kani)], replace BTreeSet/BTreeMap with Vec-based linear
// collections. Vec's push/swap_remove/contains operations model efficiently in
// Kani (no pointer chasing, no recursive tree traversal). The API is compatible
// with BTreeSet/BTreeMap, so ReactorState's method implementations need no
// conditional-compilation branches.
//
// Reference pattern: evorule-tcb/verification/fixed_map.rs (FixedMap replacing BTreeMap).
// ============================================================================

#[cfg(kani)]
mod kani_collections {
    use crate::fact::FactId;

    /// Kani-specific: Vec-based set replacing BTreeSet<FactId>
    ///
    /// API compatible with the common BTreeSet<FactId> methods (insert/remove/
    /// len/is_empty/contains). Does not guarantee iteration order (uses
    /// swap_remove), but Kani proofs do not depend on iteration order.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct KIdSet(Vec<FactId>);

    impl KIdSet {
        pub fn new() -> Self {
            Self(Vec::new())
        }
        pub fn insert(&mut self, id: FactId) -> bool {
            if self.0.iter().any(|x| x == &id) {
                return false;
            }
            self.0.push(id);
            true
        }
        pub fn remove(&mut self, id: &FactId) -> bool {
            if let Some(pos) = self.0.iter().position(|x| x == id) {
                self.0.swap_remove(pos);
                true
            } else {
                false
            }
        }
        pub fn len(&self) -> usize {
            self.0.len()
        }
        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
        pub fn contains(&self, id: &FactId) -> bool {
            self.0.iter().any(|x| x == id)
        }
    }

    /// Kani-specific: Vec-based map replacing BTreeMap<FactId, V>
    ///
    /// API compatible with the common BTreeMap<FactId, V> methods (insert/
    /// remove/len/is_empty/contains_key/get/get_mut/iter).
    #[derive(Debug, Clone, Default)]
    pub(crate) struct KIdMap<V>(Vec<(FactId, V)>);

    impl<V> KIdMap<V> {
        pub fn new() -> Self {
            Self(Vec::new())
        }
        pub fn insert(&mut self, id: FactId, val: V) -> Option<V> {
            if let Some(pos) = self.0.iter().position(|(k, _)| k == &id) {
                Some(std::mem::replace(&mut self.0[pos].1, val))
            } else {
                self.0.push((id, val));
                None
            }
        }
        pub fn remove(&mut self, id: &FactId) -> Option<V> {
            if let Some(pos) = self.0.iter().position(|(k, _)| k == id) {
                Some(self.0.swap_remove(pos).1)
            } else {
                None
            }
        }
        pub fn len(&self) -> usize {
            self.0.len()
        }
        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
        pub fn contains_key(&self, id: &FactId) -> bool {
            self.0.iter().any(|(k, _)| k == id)
        }
        pub fn get(&self, id: &FactId) -> Option<&V> {
            self.0.iter().find(|(k, _)| k == id).map(|(_, v)| v)
        }
        pub fn iter(&self) -> std::slice::Iter<'_, (FactId, V)> {
            self.0.iter()
        }
        /// Key iterator (aligns with the BTreeMap::keys consumption shape;
        /// with the Kani stand-in collections, `iter().map(|(k, _)| k)` would
        /// trigger clippy's map-keys lint)
        pub fn keys(&self) -> impl Iterator<Item = &FactId> {
            self.0.iter().map(|(k, _)| k)
        }
    }

    /// Reference iterator for KIdMap, compatible with `for (k, v) in &map`
    /// syntax (BTreeMap's IntoIterator for &BTreeMap yields (&K, &V))
    pub(crate) struct KIdMapIter<'a, V> {
        inner: std::slice::Iter<'a, (FactId, V)>,
    }

    impl<'a, V> Iterator for KIdMapIter<'a, V> {
        type Item = (&'a FactId, &'a V);
        fn next(&mut self) -> Option<Self::Item> {
            self.inner.next().map(|(k, v)| (k, v))
        }
    }

    impl<'a, V> IntoIterator for &'a KIdMap<V> {
        type Item = (&'a FactId, &'a V);
        type IntoIter = KIdMapIter<'a, V>;
        fn into_iter(self) -> Self::IntoIter {
            KIdMapIter {
                inner: self.0.iter(),
            }
        }
    }

    /// Reference iterator for KIdSet, compatible with `for x in &set` syntax
    pub(crate) struct KIdSetIter<'a> {
        inner: std::slice::Iter<'a, FactId>,
    }

    impl<'a> Iterator for KIdSetIter<'a> {
        type Item = &'a FactId;
        fn next(&mut self) -> Option<Self::Item> {
            self.inner.next()
        }
    }

    impl<'a> IntoIterator for &'a KIdSet {
        type Item = &'a FactId;
        type IntoIter = KIdSetIter<'a>;
        fn into_iter(self) -> Self::IntoIter {
            KIdSetIter {
                inner: self.0.iter(),
            }
        }
    }
}

/// Reactor internal state
#[derive(Debug, Clone)]
#[allow(dead_code)] // some utility methods serve evorule-governance or extensions
pub(crate) struct ReactorState {
    /// Current business state (payload)
    pub payload: JsonValue,

    /// Current instruction queue
    pub queue: VecDeque<JsonValue>,

    /// Monotonically increasing version number (+1 on every state change)
    pub version: u64,

    /// Previous version number (used by invariant #3: version monotonic-increase self-check)
    ///
    /// Maintained by `bump_version`: `prev_version = version; version += 1;`
    pub prev_version: u64,

    /// Number of I/O requests awaiting responses
    pub pending_io_count: usize,

    /// Set of pending I/O requests (used to validate IoResponse)
    ///
    /// Uses BTreeSet for deterministic iteration order (consistent with the
    /// evorule-tcb style).
    #[cfg(not(kani))]
    pub pending_requests: BTreeSet<FactId>,
    /// Kani mode: KIdSet replaces BTreeSet to avoid CBMC state explosion from red-black tree modeling
    #[cfg(kani)]
    pub pending_requests: kani_collections::KIdSet,

    /// Cache of the original instruction that triggered I/O (IoRequest id → (original instruction, cause))
    ///
    /// When the IoResponse arrives, the reactor re-pushes the original instruction
    /// to the front of the queue, making the `exists(__io_results__.{io_type})`
    /// dual-path mechanism in core_eval.json effective:
    /// - First execution: takes the on_false branch, triggering io_request
    /// - Resumed execution: takes the on_true branch, `set` consumes __io_results__.{io_type} into business fields
    ///
    /// Breakpoint-1 fix: also caches the cause (the cause FactId of the original
    /// instruction that triggered I/O), so that on resumed execution the cause of
    /// StateTransition/IoRequest points to the correct Fact.
    #[cfg(not(kani))]
    pub pending_io_instructions: BTreeMap<FactId, (JsonValue, FactId)>,
    /// Kani mode: KIdMap replaces BTreeMap
    #[cfg(kani)]
    pub pending_io_instructions: kani_collections::KIdMap<(JsonValue, FactId)>,

    /// Parallel cause tracking for the instruction queue (one-to-one with `queue`)
    ///
    /// Breakpoint-1 fix: every instruction in the queue is associated with a
    /// cause FactId, used for the cause field of StateTransition/IoRequest.
    /// Solves the problem of current_cause being overwritten when draining
    /// multiple Commands.
    ///
    /// Invariant: `instruction_causes.len() == queue.len()` always holds.
    pub instruction_causes: VecDeque<FactId>,

    /// I/O request emission timestamps (P3-11: used for timeout detection)
    ///
    /// Each `register_io_request` records the current time; timeout detection
    /// scans this map to determine whether the warn/error thresholds are exceeded.
    #[cfg(not(kani))]
    pub pending_io_timestamps: BTreeMap<FactId, Instant>,
    /// Kani mode: KIdMap replaces BTreeMap
    #[cfg(kani)]
    pub pending_io_timestamps: kani_collections::KIdMap<Instant>,

    /// I/O request type records (Phase 3-1.4: used to look up timeout thresholds by io_type)
    ///
    /// Each `register_io_request` records the io_type; `check_io_timeouts`
    /// looks up the dedicated thresholds of the corresponding `IoTimeoutPolicy` by io_type.
    #[cfg(not(kani))]
    pub pending_io_types: BTreeMap<FactId, IoType>,
    /// Kani mode: KIdMap replaces BTreeMap
    #[cfg(kani)]
    pub pending_io_types: kani_collections::KIdMap<IoType>,

    /// I/O recovery flag
    ///
    /// Set to true when the IoResponse arrives; after the original instruction is
    /// re-executed (execute_transition returns State), the reactor checks this flag
    /// and, if true, clears `payload.__io_results__` and resets it to false.
    /// This is necessary: in v0.3.1 the `exists` domain returns false for null
    /// (core_eval clears results with null); if io_recovery is not cleared, a later,
    /// different I/O instruction would wrongly believe it is still recovering.
    pub io_recovery: bool,

    /// Control-plane phase (white-box: makes the execution state machine observable)
    ///
    /// Updated by the main loop on every iteration, for tracing and debugging.
    /// The phase transition function is a pure algorithm (see
    /// [crate::phase::ReactorPhase::next]).
    pub phase: ReactorPhase,

    /// Cumulative count of invariant violations (white-box: structural self-check results)
    ///
    /// `check_invariants` is called on every phase transition; violations accumulate.
    /// Recorded only, never forcibly interrupts the reactor (uses `tracing::error!`
    /// instead of `debug_assert!`, per F11).
    pub structural_invariant_violations: u64,

    /// Kani-specific: __io_result__ presence flag
    ///
    /// Kani has limited ability to model BTreeMap<String, JsonValue> operations
    /// (insert/remove/contains_key) inside `JsonValue::Object`; even operating on
    /// a single key causes CBMC state explosion. Under Kani mode this flag replaces
    /// BTreeMap operations, making `inject_io_result`/`clear_io_result`/
    /// `has_io_result` O(1) operations.
    #[cfg(kani)]
    pub kani_has_io_result: bool,
}

#[allow(dead_code)] // some queue-operation methods serve extensions
impl ReactorState {
    pub fn new() -> Self {
        Self {
            payload: JsonValue::empty_object(),
            queue: VecDeque::new(),
            instruction_causes: VecDeque::new(),
            version: 0,
            prev_version: 0,
            pending_io_count: 0,
            #[cfg(not(kani))]
            pending_requests: BTreeSet::new(),
            #[cfg(kani)]
            pending_requests: kani_collections::KIdSet::new(),
            #[cfg(not(kani))]
            pending_io_instructions: BTreeMap::new(),
            #[cfg(kani)]
            pending_io_instructions: kani_collections::KIdMap::new(),
            #[cfg(not(kani))]
            pending_io_timestamps: BTreeMap::new(),
            #[cfg(kani)]
            pending_io_timestamps: kani_collections::KIdMap::new(),
            #[cfg(not(kani))]
            pending_io_types: BTreeMap::new(),
            #[cfg(kani)]
            pending_io_types: kani_collections::KIdMap::new(),
            io_recovery: false,
            phase: ReactorPhase::default(),
            structural_invariant_violations: 0,
            #[cfg(kani)]
            kani_has_io_result: false,
        }
    }

    /// Safely increment the version number (also updates prev_version, supporting the invariant #3 self-check)
    ///
    /// Replaces `state.version += 1;`, ensuring prev_version is updated in sync.
    /// Called when handling PayloadUpdate / IoResponse / StateTransition.
    pub fn bump_version(&mut self) {
        self.prev_version = self.version;
        self.version += 1;
    }

    /// Stability condition: empty queue + no pending I/O
    pub fn is_stable(&self) -> bool {
        self.queue.is_empty() && self.pending_io_count == 0
    }

    /// Returns the queue length
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Pops the front instruction together with its associated cause
    ///
    /// Breakpoint-1 fix: returns an (instruction, cause) pair; the cause is used
    /// for the cause field of the subsequent StateTransition/IoRequest Fact.
    pub fn pop_instruction(&mut self) -> Option<(JsonValue, FactId)> {
        let instr = self.queue.pop_front()?;
        let cause = self.instruction_causes.pop_front().unwrap_or(FactId(0));
        Some((instr, cause))
    }

    /// Pushes an instruction to the front of the queue, associated with the given cause
    pub fn push_front(&mut self, instruction: JsonValue, cause: FactId) {
        self.queue.push_front(instruction);
        self.instruction_causes.push_front(cause);
    }

    /// Pushes multiple instructions to the front of the queue, sharing one cause
    ///
    /// Reversing before push_front preserves the original order:
    /// [a,b,c] → push c,b,a → queue [a,b,c,...]
    pub fn push_front_all(&mut self, instructions: Vec<JsonValue>, cause: FactId) {
        for instr in instructions.into_iter().rev() {
            self.queue.push_front(instr);
            self.instruction_causes.push_front(cause);
        }
    }

    /// Pushes an instruction to the back of the queue, associated with the given cause
    pub fn push_back(&mut self, instruction: JsonValue, cause: FactId) {
        self.queue.push_back(instruction);
        self.instruction_causes.push_back(cause);
    }

    /// Pushes multiple instructions to the back of the queue, sharing one cause
    pub fn push_back_all(&mut self, instructions: Vec<JsonValue>, cause: FactId) {
        for instr in instructions {
            self.queue.push_back(instr);
            self.instruction_causes.push_back(cause);
        }
    }

    /// Clears the queue and the cause queue (cleared in sync, keeping lengths equal)
    ///
    /// Used by the MaxRoundsExceeded and queue-length-overrun recovery paths.
    ///
    /// # Kani mode
    ///
    /// CBMC models the `JsonValue` elements in a VecDeque heap buffer as arbitrary
    /// variants; `clear`'s `drop_in_place` unrolls the `Object(BTreeMap)` red-black
    /// tree destructor loop, and unbounded unwind causes state explosion (the P0-11
    /// timeout root cause). Uses `mem::take` + `forget` instead: the length-clearing
    /// semantics match the real `clear`, only skipping element destruction
    /// (same rationale as the kani_collections stand-ins).
    pub fn clear_queue(&mut self) {
        #[cfg(kani)]
        {
            let q = std::mem::take(&mut self.queue);
            std::mem::forget(q);
            let c = std::mem::take(&mut self.instruction_causes);
            std::mem::forget(c);
            return;
        }
        #[cfg(not(kani))]
        {
            self.queue.clear();
            self.instruction_causes.clear();
        }
    }

    /// Updates state with the new queue returned by execute_transition, rebuilding the cause queue in sync
    ///
    /// Breakpoint-1 fix: the push meta instruction prepends new instructions to the
    /// head of the queue, so new_queue = [newly pushed instructions...] + [remaining old instructions].
    /// - The first new_count entries are the newly pushed instructions → inherit current_cause
    /// - The remaining old_len entries are the original instructions → keep their original causes
    ///
    /// Arguments:
    /// - `new_queue`: the new queue returned by execute_transition
    /// - `current_cause`: the cause of the currently executed instruction (newly pushed instructions inherit it)
    pub fn update_queue_with_causes(&mut self, new_queue: Vec<JsonValue>, current_cause: FactId) {
        let old_len = self.queue.len();
        let old_causes: Vec<FactId> = self.instruction_causes.drain(..).collect();
        let new_count = new_queue.len().saturating_sub(old_len);
        self.queue = VecDeque::with_capacity(new_queue.len());
        for (i, instr) in new_queue.into_iter().enumerate() {
            let c = if i < new_count {
                current_cause
            } else {
                old_causes
                    .get(i - new_count)
                    .copied()
                    .unwrap_or(current_cause)
            };
            self.queue.push_back(instr);
            self.instruction_causes.push_back(c);
        }
    }

    /// Registers a pending I/O request
    ///
    /// Idempotent: re-registering the same id does not increase the count
    /// (prevents count/set-size divergence).
    /// P3-11: also records the emission timestamp for timeout detection.
    /// Phase 3-1.4: also records the io_type, for per-type timeout-threshold lookup.
    ///
    /// Compatibility wrapper: internally reads the system clock. See
    /// [`Self::register_io_request_at`] for the clock seam.
    pub fn register_io_request(&mut self, id: FactId, io_type: IoType) {
        self.register_io_request_at(id, io_type, Instant::now());
    }

    /// Time-injected variant of [`Self::register_io_request`] (clock seam)
    ///
    /// Time enters the state layer as data; the caller holds
    /// [`crate::runtime::RuntimeClock`]. Behavior is verbatim identical to the
    /// wrapper (only the time source differs).
    pub fn register_io_request_at(&mut self, id: FactId, io_type: IoType, now: Instant) {
        if self.pending_requests.insert(id) {
            self.pending_io_count += 1;
            self.pending_io_timestamps.insert(id, now);
            self.pending_io_types.insert(id, io_type);
        }
    }

    /// Queries the io_type of a given I/O request (used to locate `__io_results__.{io_type}` when handling IoResponse)
    ///
    /// Must be called before `complete_io_request`, because completing the request
    /// removes the io_type record.
    pub fn get_io_type(&self, id: &FactId) -> Option<&IoType> {
        self.pending_io_types.get(id)
    }

    /// Completes an I/O request, returning whether it succeeded (i.e. whether the request was pending)
    ///
    /// P3-11: also removes the timestamp.
    /// Phase 3-1.4: also removes the io_type record.
    pub fn complete_io_request(&mut self, id: FactId) -> bool {
        if self.pending_requests.remove(&id) {
            self.pending_io_count = self.pending_io_count.saturating_sub(1);
            self.pending_io_timestamps.remove(&id);
            self.pending_io_types.remove(&id);
            true
        } else {
            false
        }
    }

    /// Scans pending I/O for timeouts (P3-11)
    ///
    /// Returns `(warn_ids, error_ids)`:
    /// - `warn_ids`: request IDs past `warn_timeout` but not past `error_timeout`
    /// - `error_ids`: request IDs past `error_timeout` (the caller should emit Error and recover)
    pub fn scan_io_timeouts(
        &self,
        warn_timeout: Duration,
        error_timeout: Duration,
    ) -> (Vec<FactId>, Vec<FactId>) {
        self.scan_io_timeouts_at(Instant::now(), warn_timeout, error_timeout)
    }

    /// Time-injected variant of [`Self::scan_io_timeouts`] (clock seam)
    ///
    /// `now` is provided by the caller's [`crate::runtime::RuntimeClock`].
    /// The decision logic is verbatim identical to the wrapper.
    pub fn scan_io_timeouts_at(
        &self,
        now: Instant,
        warn_timeout: Duration,
        error_timeout: Duration,
    ) -> (Vec<FactId>, Vec<FactId>) {
        let mut warn_ids = Vec::new();
        let mut error_ids = Vec::new();
        // Use .iter() for compatibility with both BTreeMap and KIdMap (Kani mode)
        // BTreeMap::iter() yields (&FactId, &Instant)
        // KIdMap::iter() yields &(FactId, Instant), equivalent after for-destructuring
        for (id, timestamp) in self.pending_io_timestamps.iter() {
            let elapsed = now.duration_since(*timestamp);
            if elapsed >= error_timeout {
                error_ids.push(*id);
            } else if elapsed >= warn_timeout {
                warn_ids.push(*id);
            }
        }
        (warn_ids, error_ids)
    }

    /// Forcibly removes a timed-out I/O request (P3-11: used by error-timeout recovery)
    ///
    /// Unlike `complete_io_request`, this method does not expect an IoResponse to
    /// arrive; instead the reactor proactively cleans up the timed-out request.
    /// Also removes the cached instruction, timestamp, and io_type record.
    ///
    /// Idempotency: if the id is not in pending_requests, nothing is modified
    /// (keeping the count consistent). This ensures the
    /// `pending_io_count == pending_requests.len()` invariant holds for arbitrary id inputs.
    pub fn force_remove_io_request(&mut self, id: FactId) {
        if self.pending_requests.remove(&id) {
            self.pending_io_count = self.pending_io_count.saturating_sub(1);
            self.pending_io_instructions.remove(&id);
            self.pending_io_timestamps.remove(&id);
            self.pending_io_types.remove(&id);
        }
    }

    /// Caches the original instruction that triggered I/O and its cause (called when an IoRequest is produced)
    ///
    /// When the IoResponse arrives, `take_io_instruction` retrieves it and re-pushes
    /// it to the front of the queue, making the dual-path mechanism in core_eval.json effective.
    ///
    /// Breakpoint-1 fix: also caches the cause, so on resumed execution the cause
    /// points to the correct Fact.
    pub fn save_io_instruction(&mut self, id: FactId, instruction: JsonValue, cause: FactId) {
        self.pending_io_instructions
            .insert(id, (instruction, cause));
    }

    /// Takes and removes the cached original instruction and its cause (called when handling IoResponse)
    ///
    /// Returns `Some((instruction, cause))` if the id has a cache entry and it should
    /// be re-pushed to the queue; returns `None` if the id has no cache entry
    /// (possibly an unknown IoResponse or duplicate handling).
    pub fn take_io_instruction(&mut self, id: FactId) -> Option<(JsonValue, FactId)> {
        self.pending_io_instructions.remove(&id)
    }

    /// Clears the `__io_results__` field from the payload
    ///
    /// v0.3.1: I/O results are stored per io_type in `__io_results__.{io_type}`;
    /// after core_eval consumes them it sets the corresponding entry to null
    /// (`exists` treats null as absent). Called after I/O-resumed execution to
    /// remove the whole `__io_results__` container, preventing leftover null
    /// entries from being misread by later I/O instructions and avoiding stale
    /// results affecting later I/O instructions. Since at most one I/O is pending
    /// at any moment, removing the whole container is safe.
    pub fn clear_io_result(&mut self) {
        #[cfg(kani)]
        {
            self.kani_has_io_result = false;
            return;
        }
        #[cfg(not(kani))]
        {
            if let JsonValue::Object(map) = &mut self.payload {
                map.remove("__io_results__");
            }
        }
    }

    /// Atomically clears the I/O recovery state (clears both `__io_results__` and the `io_recovery` flag)
    ///
    /// This method performs both operations in a single call, avoiding intermediate
    /// states that violate invariants #2/#4.
    /// Note: invariant checks are called at the start of the 'main loop and are not
    /// triggered inside this method, so intermediate states are not misreported.
    pub fn clear_io_recovery(&mut self) {
        self.clear_io_result();
        self.io_recovery = false;
    }
}

impl Default for ReactorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_state_initial() {
        let state = ReactorState::new();
        assert!(state.is_stable());
        assert_eq!(state.version, 0);
        assert_eq!(state.pending_io_count, 0);
        assert!(state.pending_requests.is_empty());
    }

    #[test]
    fn test_io_request_tracking() {
        let mut state = ReactorState::new();
        let id1 = FactId(1);
        let id2 = FactId(2);

        state.register_io_request(id1, IoType::call_external());
        assert_eq!(state.pending_io_count, 1);
        assert!(state.pending_requests.contains(&id1));

        state.register_io_request(id2, IoType::call_external());
        assert_eq!(state.pending_io_count, 2);
        assert!(state.pending_requests.contains(&id2));

        assert!(state.complete_io_request(id1));
        assert_eq!(state.pending_io_count, 1);
        assert!(!state.pending_requests.contains(&id1));

        // Completing a non-existent request
        assert!(!state.complete_io_request(FactId(999)));
        assert_eq!(state.pending_io_count, 1);
    }

    #[test]
    fn test_push_front_all_preserves_order() {
        let mut state = ReactorState::new();
        let instrs = vec![
            JsonValue::string("a"),
            JsonValue::string("b"),
            JsonValue::string("c"),
        ];
        state.push_front_all(instrs, FactId(1));

        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("a"), FactId(1)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("b"), FactId(1)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("c"), FactId(1)))
        );
    }

    #[test]
    fn test_push_back_all_preserves_order() {
        let mut state = ReactorState::new();
        let instrs = vec![
            JsonValue::string("x"),
            JsonValue::string("y"),
            JsonValue::string("z"),
        ];
        state.push_back_all(instrs, FactId(2));

        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("x"), FactId(2)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("y"), FactId(2)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("z"), FactId(2)))
        );
    }

    #[test]
    fn test_push_front_single() {
        let mut state = ReactorState::new();
        state.push_back(JsonValue::string("a"), FactId(1));
        state.push_back(JsonValue::string("b"), FactId(2));

        // push_front inserts the instruction at the front of the queue
        state.push_front(JsonValue::string("urgent"), FactId(3));

        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("urgent"), FactId(3)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("a"), FactId(1)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("b"), FactId(2)))
        );
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_push_front_all_to_empty_queue() {
        let mut state = ReactorState::new();
        state.push_front_all(vec![JsonValue::string("only")], FactId(1));
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("only"), FactId(1)))
        );
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_push_front_all_interleaved_with_existing() {
        let mut state = ReactorState::new();
        state.push_back(JsonValue::string("old"), FactId(1));

        // push_front_all should insert the new instructions before "old"
        state.push_front_all(
            vec![JsonValue::string("new1"), JsonValue::string("new2")],
            FactId(2),
        );

        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("new1"), FactId(2)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("new2"), FactId(2)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("old"), FactId(1)))
        );
    }

    #[test]
    fn test_register_duplicate_io_request() {
        // Re-registering the same id must be idempotent: count and set size stay consistent
        let mut state = ReactorState::new();
        let id = FactId(1);

        state.register_io_request(id, IoType::call_external());
        assert_eq!(state.pending_io_count, 1);
        assert_eq!(state.pending_requests.len(), 1);

        // Re-registering the same id: count unchanged (idempotent)
        state.register_io_request(id, IoType::call_external());
        assert_eq!(state.pending_io_count, 1);
        assert_eq!(state.pending_requests.len(), 1);

        // After completion: count drops to 0, set is empty
        assert!(state.complete_io_request(id));
        assert_eq!(state.pending_io_count, 0);
        assert!(state.pending_requests.is_empty());

        // Completing again: returns false
        assert!(!state.complete_io_request(id));
        assert_eq!(state.pending_io_count, 0);
    }

    #[test]
    fn test_queue_len() {
        let mut state = ReactorState::new();
        assert_eq!(state.queue_len(), 0);

        state.push_back(JsonValue::string("a"), FactId(1));
        assert_eq!(state.queue_len(), 1);

        state.push_back(JsonValue::string("b"), FactId(2));
        assert_eq!(state.queue_len(), 2);

        state.pop_instruction();
        assert_eq!(state.queue_len(), 1);
    }

    #[test]
    fn test_pop_from_empty_queue() {
        let mut state = ReactorState::new();
        assert_eq!(state.pop_instruction(), None);
    }

    #[test]
    fn test_instruction_causes_synced_with_queue() {
        // Breakpoint 1: verify instruction_causes always stays in sync with queue
        let mut state = ReactorState::new();
        assert_eq!(state.instruction_causes.len(), state.queue.len());

        state.push_back(JsonValue::string("a"), FactId(10));
        state.push_back(JsonValue::string("b"), FactId(20));
        assert_eq!(state.instruction_causes.len(), state.queue.len());

        state.pop_instruction();
        assert_eq!(state.instruction_causes.len(), state.queue.len());

        state.clear_queue();
        assert_eq!(state.instruction_causes.len(), state.queue.len());
        assert_eq!(state.instruction_causes.len(), 0);
    }

    #[test]
    fn test_update_queue_with_causes_new_push() {
        // Breakpoint 1: verify update_queue_with_causes assigns causes correctly
        // Simulation: after popping, the queue holds [B, C] (causes=[cB, cC]);
        // execute_transition pushed [X, Y], returning new_queue = [X, Y, B, C]
        let mut state = ReactorState::new();
        state.push_back(JsonValue::string("B"), FactId(2));
        state.push_back(JsonValue::string("C"), FactId(3));
        // At this point queue = [B, C], causes = [2, 3]

        // Simulate the cause after pop
        let current_cause = FactId(1); // cause of the popped instruction

        // new_queue = [X, Y, B, C] (push prepended X, Y)
        let new_queue = vec![
            JsonValue::string("X"),
            JsonValue::string("Y"),
            JsonValue::string("B"),
            JsonValue::string("C"),
        ];
        state.update_queue_with_causes(new_queue, current_cause);

        // Verify: X, Y inherit current_cause(1); B, C keep their original causes (2, 3)
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("X"), FactId(1)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("Y"), FactId(1)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("B"), FactId(2)))
        );
        assert_eq!(
            state.pop_instruction(),
            Some((JsonValue::string("C"), FactId(3)))
        );
    }

    #[test]
    fn test_complete_io_request_empty() {
        let mut state = ReactorState::new();
        // Completing any id on an empty state should return false
        assert!(!state.complete_io_request(FactId(999)));
        assert_eq!(state.pending_io_count, 0);
    }

    #[test]
    fn test_default_state() {
        let state = ReactorState::default();
        assert!(state.is_stable());
        assert_eq!(state.version, 0);
        assert_eq!(state.queue_len(), 0);
    }

    // ===== P3-11 resource-management tests =====

    #[test]
    fn test_register_io_request_records_timestamp() {
        // P3-11: register_io_request should also record the timestamp
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        assert!(state.pending_io_timestamps.contains_key(&id));
    }

    #[test]
    fn test_complete_io_request_removes_timestamp() {
        // P3-11: complete_io_request should also remove the timestamp
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        assert!(state.pending_io_timestamps.contains_key(&id));
        assert!(state.complete_io_request(id));
        assert!(!state.pending_io_timestamps.contains_key(&id));
    }

    #[test]
    fn test_scan_io_timeouts_no_pending() {
        // P3-11: scan returns empty when there is no pending I/O
        let state = ReactorState::new();
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert!(warn_ids.is_empty());
        assert!(error_ids.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_warn_level() {
        // P3-11: past warn_timeout but not past error_timeout
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        // Simulate a request registered 35s ago (over the 30s warn, under the 60s error)
        state
            .pending_io_timestamps
            .insert(id, Instant::now() - Duration::from_secs(35));
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert_eq!(warn_ids, vec![FactId(1)]);
        assert!(error_ids.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_error_level() {
        // P3-11: past error_timeout
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        // Simulate a request registered 65s ago (over the 60s error)
        state
            .pending_io_timestamps
            .insert(id, Instant::now() - Duration::from_secs(65));
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        // Requests over the error threshold do not also appear in the warn list
        assert!(warn_ids.is_empty());
        assert_eq!(error_ids, vec![FactId(1)]);
    }

    #[test]
    fn test_scan_io_timeouts_mixed() {
        // P3-11: mixed scenario — one at warn level, one at error level, one normal
        let mut state = ReactorState::new();
        let warn_id = FactId(1);
        let error_id = FactId(2);
        let normal_id = FactId(3);
        state.register_io_request(warn_id, IoType::call_external());
        state.register_io_request(error_id, IoType::call_external());
        state.register_io_request(normal_id, IoType::call_external());
        // Set timestamps manually
        state
            .pending_io_timestamps
            .insert(warn_id, Instant::now() - Duration::from_secs(40));
        state
            .pending_io_timestamps
            .insert(error_id, Instant::now() - Duration::from_secs(70));
        // normal_id keeps the current time
        let (warn_ids, error_ids) =
            state.scan_io_timeouts(Duration::from_secs(30), Duration::from_secs(60));
        assert_eq!(warn_ids, vec![FactId(1)]);
        assert_eq!(error_ids, vec![FactId(2)]);
    }

    #[test]
    fn test_force_remove_io_request() {
        // P3-11 + Phase 3-1.4: force_remove_io_request should clear all related state
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        state.save_io_instruction(id, JsonValue::string("original_instruction"), FactId(100));

        // Confirm the state has been set
        assert_eq!(state.pending_io_count, 1);
        assert!(state.pending_requests.contains(&id));
        assert!(state.pending_io_instructions.contains_key(&id));
        assert!(state.pending_io_timestamps.contains_key(&id));
        assert!(state.pending_io_types.contains_key(&id));

        // Force removal
        state.force_remove_io_request(id);

        // Confirm all related state has been cleared
        assert_eq!(state.pending_io_count, 0);
        assert!(!state.pending_requests.contains(&id));
        assert!(!state.pending_io_instructions.contains_key(&id));
        assert!(!state.pending_io_timestamps.contains_key(&id));
        assert!(!state.pending_io_types.contains_key(&id));
    }

    #[test]
    fn test_force_remove_nonexistent_io_request() {
        // P3-11: forcibly removing a non-existent request must not panic
        let mut state = ReactorState::new();
        state.force_remove_io_request(FactId(999));
        assert_eq!(state.pending_io_count, 0);
    }

    // ===== Phase 3-1.4 I/O type recording tests =====

    #[test]
    fn test_register_io_request_records_io_type() {
        // Phase 3-1.4: register_io_request should also record the io_type
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::query_db());
        assert_eq!(state.pending_io_types.get(&id), Some(&IoType::query_db()));
    }

    #[test]
    fn test_complete_io_request_removes_io_type() {
        // Phase 3-1.4: complete_io_request should also remove the io_type record
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::call_external());
        assert!(state.pending_io_types.contains_key(&id));
        assert!(state.complete_io_request(id));
        assert!(!state.pending_io_types.contains_key(&id));
    }

    #[test]
    fn test_register_multiple_io_types() {
        // Phase 3-1.4: different requests record different io_types
        let mut state = ReactorState::new();
        state.register_io_request(FactId(1), IoType::call_external());
        state.register_io_request(FactId(2), IoType::query_db());
        state.register_io_request(FactId(3), IoType::http_get());

        assert_eq!(state.pending_io_types.len(), 3);
        assert_eq!(
            state.pending_io_types.get(&FactId(1)),
            Some(&IoType::call_external())
        );
        assert_eq!(
            state.pending_io_types.get(&FactId(2)),
            Some(&IoType::query_db())
        );
        assert_eq!(
            state.pending_io_types.get(&FactId(3)),
            Some(&IoType::http_get())
        );
    }
}
