// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Permission entry data model
//!
//! Defines [`PermissionEntry`] and its constituent types; the schema aligns with implementation doc 18.
//! This module is a pure data model (serde-driven) and contains no decision logic.

use serde::{Deserialize, Serialize};

/// Permission entry lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
    /// Draft: being edited by the creator, not yet submitted for approval
    #[default]
    Draft,
    /// Candidate: submitted, awaiting the approver's decision
    Candidate,
    /// Active: approved and entered into the decision set
    Active,
    /// Rejected | voided (removed from the effective decision set)
    Rejected,
}

impl PermissionState {
    /// Whether in the "effective decision" state (only `Active` participates in decisions)
    pub const fn is_active(self) -> bool {
        matches!(self, PermissionState::Active)
    }

    /// Whether in the "pending approval" state (`Candidate`)
    pub const fn is_candidate(self) -> bool {
        matches!(self, PermissionState::Candidate)
    }
}

/// Decision effect: allow or deny
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Allow
    #[default]
    Allow,
    /// Deny
    Deny,
}

/// Subject type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubjectType {
    /// Named user
    User,
    /// Role
    Role,
    /// Rule file (author)
    Rule,
    /// LLM agent
    LlmAgent,
    /// Any subject (wildcard)
    #[default]
    Any,
}

/// Permission subject: who is allowed/denied
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Subject {
    /// Subject type
    #[serde(default)]
    pub subject_type: SubjectType,
    /// Subject identifier (empty string when `type = Any`)
    #[serde(default)]
    pub id: String,
}

impl Subject {
    /// Wildcard subject (matches any caller)
    pub fn any() -> Self {
        Self {
            subject_type: SubjectType::Any,
            id: String::new(),
        }
    }

    /// Human rule-author subject
    pub fn human(name: impl Into<String>) -> Self {
        Self {
            subject_type: SubjectType::User,
            id: name.into(),
        }
    }
}

/// Resource type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// Fact (state/queue/shared paths)
    Fact,
    /// I/O action
    IoAction,
    /// HTTP API
    Api,
    /// Shared-facts domain
    #[default]
    Shared,
}

/// Permission resource: what to protect
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Resource {
    /// Resource type
    #[serde(default)]
    pub resource_type: ResourceType,
    /// Resource match pattern (trailing `*` = prefix wildcard), e.g. `shared.*` / `db.users.*`
    #[serde(default)]
    pub path: String,
}

/// Scope: limits the tenant range in effect
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Scope {
    /// Tenant ID in effect; `None` = globally effective
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

/// Permission entry — one subject→resource→action allow/deny declaration
///
/// The data model aligns with implementation doc 18. Stored as pure data in TCB JSON under
/// `SharedFactsLog`, and decided after [`super::PermissionTable`] rebuilds the snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionEntry {
    /// Entry ID (the `<id>` part of the storage path `shared.__permission__.entry.<id>`)
    pub id: String,
    /// Version (overwritten with the latest global version on read, for replay alignment)
    #[serde(default)]
    pub version: u64,
    /// Lifecycle state
    #[serde(default)]
    pub state: PermissionState,
    /// Subject
    #[serde(default)]
    pub subject: Subject,
    /// Resource
    #[serde(default)]
    pub resource: Resource,
    /// Action identifier (corresponds to the actual I/O instruction type / API name)
    #[serde(default = "default_action")]
    pub action: String,
    /// Decision effect
    #[serde(default)]
    pub effect: Effect,
    /// Condition expression (optional, handed to TCB `evaluate_domain`); structure see [`super::condition`]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<serde_json::Value>,
    /// Scope
    #[serde(default)]
    pub scope: Scope,
    /// Trigger origin (for approval/audit traceability)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// Last modified by
    #[serde(default = "default_updated_by")]
    pub updated_by: String,
}

/// Default value of the `action` field (`*` = any action)
fn default_action() -> String {
    "*".to_string()
}

/// Default value of the `updated_by` field
fn default_updated_by() -> String {
    "unknown".to_string()
}

impl PermissionEntry {
    /// Constructs a new entry (version always 1, state always `Draft`, action wildcard)
    pub fn new(
        id: impl Into<String>,
        subject: Subject,
        resource: Resource,
        effect: Effect,
    ) -> Self {
        Self {
            id: id.into(),
            version: 1,
            state: PermissionState::Draft,
            subject,
            resource,
            action: default_action(),
            effect,
            conditions: None,
            scope: Scope { tenant_id: None },
            cause: None,
            updated_by: default_updated_by(),
        }
    }

    /// Enters the candidate (pending-approval) state
    pub fn submit(&mut self) -> Result<(), PermissionError> {
        self.state = match self.state {
            PermissionState::Draft => PermissionState::Candidate,
            other => {
                return Err(PermissionError::InvalidState(format!(
                    "only Draft can be submitted, current = {other:?}"
                )))
            }
        };
        Ok(())
    }

    /// Approval decision: approve activates, deny voids
    pub fn review(&mut self, approve: bool) -> Result<(), PermissionError> {
        match self.state {
            PermissionState::Candidate => {
                self.state = if approve {
                    PermissionState::Active
                } else {
                    PermissionState::Rejected
                };
                Ok(())
            }
            other => Err(PermissionError::InvalidState(format!(
                "only Candidate can be reviewed, current = {other:?}"
            ))),
        }
    }
}

/// Permission subsystem errors
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    /// Entry not found
    #[error("permission entry not found: {0}")]
    NotFound(String),
    /// Invalid state
    #[error("invalid permission state: {0}")]
    InvalidState(String),
    /// Condition evaluation failed
    #[error("condition evaluation failed: {0}")]
    Condition(String),
    /// Serialization/deserialization failed
    #[error("permission serialization error: {0}")]
    Json(#[from] serde_json::Error),
    /// Storage-layer error
    #[error("permission store error: {0}")]
    Store(String),
}
