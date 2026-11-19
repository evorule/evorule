// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor execution phases (making the control-plane state machine explicit)
//!
//! # Design
//!
//! ReactorPhase is the reactor's **control-plane state**, describing "what is
//! being done now":
//! - Not business semantics ("computing call_llm"), but control semantics
//!   ("waiting for I/O")
//! - The transition function is a pure algorithm, unit-testable
//! - The main loop is driven by phase: observable and evolvable
//!
//! # Spec compliance
//!
//! - Phase is a control-plane concept, not business semantics (per G8)
//! - The transition function is a pure algorithm (per mechanism-policy separation)
//! - Contains no business-term strings (per §5.2)
//! - Nesting at most 2 levels (per F8)
//! - Single function no more than 50 lines (per F9)

/// Reactor execution phase
///
/// Describes the control-plane state the reactor main loop is currently in.
/// Updated on every main-loop iteration, for observability and debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReactorPhase {
    /// After startup or a Stable round, waiting for the first Fact
    #[default]
    Idle,
    /// Non-blocking drain of all pending Facts from the command channel
    Draining,
    /// Continuously executing queue instructions (pending_io == 0)
    Executing,
    /// Waiting for I/O responses (pending_io > 0)
    AwaitingIo,
    /// Stable in this round; emits the Stable Fact then returns to Idle
    Stable,
    /// Error state (max_rounds/TCB/I-O timeout); returns to Idle after recovery
    Error,
}

impl ReactorPhase {
    /// Phase name (for tracing and Prometheus labels)
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Draining => "draining",
            Self::Executing => "executing",
            Self::AwaitingIo => "awaiting_io",
            Self::Stable => "stable",
            Self::Error => "error",
        }
    }
}

/// Phase transition context (pure data, used by the transition function)
///
/// Key information extracted from ReactorState, making the transition function pure.
#[derive(Debug, Clone)]
pub struct PhaseContext {
    /// Whether the queue is empty
    pub queue_empty: bool,
    /// Number of pending I/O requests
    pub pending_io: usize,
    /// Number of executed steps
    pub steps: usize,
    /// Whether this round drained any Fact
    pub drained_any: bool,
}

impl ReactorPhase {
    /// Phase transition function (pure algorithm, unit-testable)
    ///
    /// Given the current phase and context, returns the next phase.
    /// This function modifies no state; it only makes logical decisions.
    pub fn next(self, ctx: &PhaseContext) -> Self {
        match self {
            Self::Idle => Self::Draining,
            Self::Draining => Self::next_after_draining(ctx),
            Self::Executing => Self::next_after_executing(ctx),
            Self::AwaitingIo | Self::Stable | Self::Error => Self::Idle,
        }
    }

    /// Transition after Draining: decides the next phase from the context
    fn next_after_draining(ctx: &PhaseContext) -> Self {
        if ctx.pending_io > 0 {
            Self::AwaitingIo
        } else if !ctx.queue_empty {
            Self::Executing
        } else if ctx.steps > 0 {
            Self::Stable
        } else {
            Self::Idle
        }
    }

    /// Transition after Executing: decides the next phase from the context
    fn next_after_executing(ctx: &PhaseContext) -> Self {
        if ctx.pending_io > 0 {
            Self::AwaitingIo
        } else if ctx.queue_empty {
            Self::post_execution(ctx)
        } else {
            Self::Executing
        }
    }

    /// Transition after execution finishes: decides stable or back to Idle
    fn post_execution(ctx: &PhaseContext) -> Self {
        if ctx.steps > 0 {
            Self::Stable
        } else {
            Self::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(queue_empty: bool, pending_io: usize, steps: usize, drained_any: bool) -> PhaseContext {
        PhaseContext {
            queue_empty,
            pending_io,
            steps,
            drained_any,
        }
    }

    #[test]
    fn test_phase_idle_to_draining() {
        let phase = ReactorPhase::Idle;
        let c = ctx(true, 0, 0, false);
        assert_eq!(phase.next(&c), ReactorPhase::Draining);
    }

    #[test]
    fn test_phase_draining_to_executing_when_queue_nonempty() {
        let phase = ReactorPhase::Draining;
        let c = ctx(false, 0, 0, true);
        assert_eq!(phase.next(&c), ReactorPhase::Executing);
    }

    #[test]
    fn test_phase_draining_to_awaiting_io() {
        let phase = ReactorPhase::Draining;
        let c = ctx(true, 2, 5, true);
        assert_eq!(phase.next(&c), ReactorPhase::AwaitingIo);
    }

    #[test]
    fn test_phase_draining_to_stable() {
        let phase = ReactorPhase::Draining;
        let c = ctx(true, 0, 10, true);
        assert_eq!(phase.next(&c), ReactorPhase::Stable);
    }

    #[test]
    fn test_phase_draining_to_idle_when_no_work() {
        let phase = ReactorPhase::Draining;
        let c = ctx(true, 0, 0, false);
        assert_eq!(phase.next(&c), ReactorPhase::Idle);
    }

    #[test]
    fn test_phase_executing_to_awaiting_io() {
        let phase = ReactorPhase::Executing;
        let c = ctx(false, 1, 3, true);
        assert_eq!(phase.next(&c), ReactorPhase::AwaitingIo);
    }

    #[test]
    fn test_phase_executing_to_stable_when_queue_empty() {
        let phase = ReactorPhase::Executing;
        let c = ctx(true, 0, 5, true);
        assert_eq!(phase.next(&c), ReactorPhase::Stable);
    }

    #[test]
    fn test_phase_executing_continues_when_queue_nonempty() {
        let phase = ReactorPhase::Executing;
        let c = ctx(false, 0, 3, true);
        assert_eq!(phase.next(&c), ReactorPhase::Executing);
    }

    #[test]
    fn test_phase_awaiting_io_to_idle() {
        let phase = ReactorPhase::AwaitingIo;
        let c = ctx(false, 0, 5, true);
        assert_eq!(phase.next(&c), ReactorPhase::Idle);
    }

    #[test]
    fn test_phase_stable_to_idle() {
        let phase = ReactorPhase::Stable;
        let c = ctx(true, 0, 0, false);
        assert_eq!(phase.next(&c), ReactorPhase::Idle);
    }

    #[test]
    fn test_phase_error_to_idle() {
        let phase = ReactorPhase::Error;
        let c = ctx(true, 0, 0, false);
        assert_eq!(phase.next(&c), ReactorPhase::Idle);
    }

    #[test]
    fn test_phase_as_str() {
        assert_eq!(ReactorPhase::Idle.as_str(), "idle");
        assert_eq!(ReactorPhase::Draining.as_str(), "draining");
        assert_eq!(ReactorPhase::Executing.as_str(), "executing");
        assert_eq!(ReactorPhase::AwaitingIo.as_str(), "awaiting_io");
        assert_eq!(ReactorPhase::Stable.as_str(), "stable");
        assert_eq!(ReactorPhase::Error.as_str(), "error");
    }
}
