// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor error types

/// Reactor error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactorError {
    /// Channel closed (all senders dropped)
    ChannelClosed,

    /// Maximum rounds limit reached
    MaxRoundsExceeded {
        /// Actual number of rounds
        rounds: usize,
        /// Maximum allowed number of rounds
        max_rounds: usize,
    },

    /// TCB execution error (contains the original error message)
    TcbError {
        /// Error description
        message: String,
    },

    /// Invalid state (payload or queue missing)
    InvalidState {
        /// The missing field
        field: &'static str,
    },

    /// Cancel signal (externally requested stop)
    Cancelled,

    /// Reactor task terminated abnormally (panicked or aborted)
    TaskJoinError {
        /// Error description
        message: String,
    },
}

impl core::fmt::Display for ReactorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReactorError::ChannelClosed => write!(f, "fact channel closed"),
            ReactorError::MaxRoundsExceeded { rounds, max_rounds } => {
                write!(f, "max rounds exceeded: {} > {}", rounds, max_rounds)
            }
            ReactorError::TcbError { message } => write!(f, "TCB error: {}", message),
            ReactorError::InvalidState { field } => write!(f, "invalid state: missing {}", field),
            ReactorError::Cancelled => write!(f, "reactor cancelled"),
            ReactorError::TaskJoinError { message } => write!(f, "task join error: {}", message),
        }
    }
}

impl std::error::Error for ReactorError {}

impl From<evorule_tcb::TcbError> for ReactorError {
    fn from(err: evorule_tcb::TcbError) -> Self {
        ReactorError::TcbError {
            message: err.to_string(),
        }
    }
}
