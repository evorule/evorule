// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TCB termination envelope parameters (configurable caps of the
//! termination defenses).
//!
//! The four envelope constants (transform-rule cap / execution budget /
//! branch depth / domain depth) jointly constitute the termination
//! guarantee (SPEC T6 / M6). This module converges the four caps into one
//! parameter struct so deployments can tune them per environment (tighten
//! on resource-constrained devices, widen for large rule sets).
//!
//! # Default invariant
//!
//! [`EnvelopeLimits::default`] equals the four `pub const` current values
//! field-by-field (128 / 2048 / 64 / 64) —
//! [`execute_transition`](crate::execute_transition) runs on the default
//! envelope and behaves byte-identically to the pre-parameterization code.
//!
//! # Sizing invariant (fail-fast validation)
//!
//! The execution budget must be >= rule cap x [`META_INSTRUCTIONS_PER_RULE`]
//! (default 2048 = 128 x 16): this preserves the acceptance-boundary
//! consistency that "a rule set admitted by the load gate can never hit the
//! execution budget at runtime" (parameterized form of the v0.13.0
//! lockstep semantics). Violating constructions/uses return `Err` — no
//! guessing, no silent fallback.
//!
//! # Upper bound and load-gate boundary
//!
//! This struct imposes no usize hard cap; when `max_transform_rules` is
//! widened beyond 128, the load-side JSON schema (rule_set v1.1
//! `maxItems: 128`) rejects first — schema widening requires a new schema
//! version legislation and is out of this struct's scope.
//!
//! # Error shape note
//!
//! Validation failures reuse [`TcbError::InvalidState`] (reason string
//! carries details) instead of adding a new enum variant: `TcbError` is
//! not `#[non_exhaustive]`, and keeping the variant set frozen is part of
//! the zero-breakage promise for downstream exhaustive matches.

use crate::domain::MAX_DOMAIN_DEPTH;
use crate::error::TcbError;
use crate::executor::{MAX_BRANCH_DEPTH, MAX_TOTAL_META_INSTRUCTIONS};
use crate::transition::MAX_TRANSFORM_RULES;
use alloc::format;

/// Budget multiplier per rule for the sizing invariant (default 16)
///
/// Same source as the executor.rs budget doc "rule cap x avg instructions
/// per rule (<=16)"; kept as a constant, not parameterized — the
/// multiplier is the budget sizing basis, not a deployment-tunable knob.
pub const META_INSTRUCTIONS_PER_RULE: usize = 16;

/// Termination envelope parameters (converged structure of four caps)
///
/// Fields are public and readable; prefer [`EnvelopeLimits::new`] for
/// construction (fail-fast validation). Hand-built field combinations are
/// re-validated at the
/// [`execute_transition_with_limits`](crate::transition::execute_transition_with_limits)
/// entry (fail-fast; construction paths are not trusted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeLimits {
    /// `core_eval` rule-count cap (default [`MAX_TRANSFORM_RULES`] = 128)
    pub max_transform_rules: usize,
    /// Total executed meta-instruction cap per transition (default [`MAX_TOTAL_META_INSTRUCTIONS`] = 2048)
    pub max_total_meta_instructions: usize,
    /// Branch nesting depth cap (default [`MAX_BRANCH_DEPTH`] = 64)
    pub max_branch_depth: usize,
    /// Domain evaluation recursion depth cap (default [`MAX_DOMAIN_DEPTH`] = 64; 1 under Kani builds)
    pub max_domain_depth: usize,
}

impl Default for EnvelopeLimits {
    fn default() -> Self {
        Self {
            max_transform_rules: MAX_TRANSFORM_RULES,
            max_total_meta_instructions: MAX_TOTAL_META_INSTRUCTIONS,
            max_branch_depth: MAX_BRANCH_DEPTH,
            max_domain_depth: MAX_DOMAIN_DEPTH,
        }
    }
}

impl EnvelopeLimits {
    /// Construct and validate (fail-fast: violations return `Err`, no silent fallback)
    ///
    /// # Errors
    ///
    /// Sizing-invariant violations (any cap is 0, or budget < rule cap x
    /// [`META_INSTRUCTIONS_PER_RULE`]) return `TcbError::InvalidState`.
    pub fn new(
        max_transform_rules: usize,
        max_total_meta_instructions: usize,
        max_branch_depth: usize,
        max_domain_depth: usize,
    ) -> Result<Self, TcbError> {
        let limits = Self {
            max_transform_rules,
            max_total_meta_instructions,
            max_branch_depth,
            max_domain_depth,
        };
        limits.validate()?;
        Ok(limits)
    }

    /// Validate the sizing invariants (re-checked at every
    /// `execute_transition_with_limits` entry)
    ///
    /// # Errors
    ///
    /// Same conditions as [`EnvelopeLimits::new`].
    pub fn validate(&self) -> Result<(), TcbError> {
        let zero = |name: &str| TcbError::InvalidState {
            reason: format!("envelope limit {name} must be >= 1"),
        };
        if self.max_transform_rules == 0 {
            return Err(zero("max_transform_rules"));
        }
        if self.max_total_meta_instructions == 0 {
            return Err(zero("max_total_meta_instructions"));
        }
        if self.max_branch_depth == 0 {
            return Err(zero("max_branch_depth"));
        }
        if self.max_domain_depth == 0 {
            return Err(zero("max_domain_depth"));
        }
        let min_budget = self
            .max_transform_rules
            .saturating_mul(META_INSTRUCTIONS_PER_RULE);
        if self.max_total_meta_instructions < min_budget {
            return Err(TcbError::InvalidState {
                reason: format!(
                    "max_total_meta_instructions ({}) must be >= max_transform_rules ({}) x META_INSTRUCTIONS_PER_RULE ({}) = {}",
                    self.max_total_meta_instructions,
                    self.max_transform_rules,
                    META_INSTRUCTIONS_PER_RULE,
                    min_budget
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::transition::{execute_transition, execute_transition_with_limits, TransitionResult};
    use crate::value::JsonValue;
    extern crate alloc;
    use alloc::vec;
    use alloc::vec::Vec;

    // ---- builders ----

    fn make_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
    }

    fn make_payload() -> JsonValue {
        JsonValue::object_from_pairs(&[("x", JsonValue::Integer(0))])
    }

    fn set_rule(attr: &str, value: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(attr)),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(value)),
                ]),
            ),
        ])
    }

    fn branch_rule(domain: JsonValue, on_true: Vec<JsonValue>) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("domain", domain),
                    ("on_true", JsonValue::array(on_true)),
                ]),
            ),
        ])
    }

    fn enforce_rule(domain: JsonValue) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("enforce")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("domain", domain),
                    ("reason", JsonValue::string("test")),
                ]),
            ),
        ])
    }

    fn eq_domain() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(0)),
        ])
    }

    /// Nested `all` domain: `nest` layers of all (inner arrays), leaf is eq(x == 0) (always true)
    fn nested_all_domain(nest: usize) -> JsonValue {
        let mut domain = eq_domain();
        for _ in 0..nest {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                ("inner", JsonValue::array(vec![domain])),
            ]);
        }
        domain
    }

    fn run_transition(limits: &EnvelopeLimits, core_eval: Vec<JsonValue>) -> TransitionResult {
        execute_transition_with_limits(
            &core_eval,
            &make_instruction(),
            &make_payload(),
            &[],
            limits,
        )
        .unwrap_or_else(|e| panic!("transition failed unexpectedly: {e:?}"))
    }

    // ---- default invariant: Default equals the current pub consts field-by-field (drift lock) ----

    #[test]
    fn default_equals_published_consts() {
        let d = EnvelopeLimits::default();
        assert_eq!(d.max_transform_rules, MAX_TRANSFORM_RULES);
        assert_eq!(d.max_total_meta_instructions, MAX_TOTAL_META_INSTRUCTIONS);
        assert_eq!(d.max_branch_depth, MAX_BRANCH_DEPTH);
        assert_eq!(d.max_domain_depth, MAX_DOMAIN_DEPTH);
    }

    // ---- fail-fast validation (construction and entry, both sides) ----

    #[test]
    fn new_rejects_zero_limits() {
        assert!(matches!(
            EnvelopeLimits::new(0, 2048, 64, 64),
            Err(TcbError::InvalidState { .. })
        ));
        assert!(matches!(
            EnvelopeLimits::new(128, 2048, 0, 64),
            Err(TcbError::InvalidState { .. })
        ));
        assert!(matches!(
            EnvelopeLimits::new(128, 2048, 64, 0),
            Err(TcbError::InvalidState { .. })
        ));
    }

    #[test]
    fn new_rejects_budget_below_sizing_invariant() {
        // 100 < 128 x 16 = 2048: an admitted rule set could hit the budget; reject construction
        assert!(matches!(
            EnvelopeLimits::new(128, 100, 64, 64),
            Err(TcbError::InvalidState { .. })
        ));
        // boundary 2048 == 128 x 16: exactly the minimum, accepted
        assert!(EnvelopeLimits::new(128, 2048, 64, 64).is_ok());
    }

    #[test]
    fn entry_revalidates_hand_built_invalid_limits() {
        // hand-built invalid combination bypassing new(); rejected fail-fast at entry
        let limits = EnvelopeLimits {
            max_transform_rules: 8,
            max_total_meta_instructions: 100,
            max_branch_depth: 64,
            max_domain_depth: 64,
        };
        let result = execute_transition_with_limits(
            &[set_rule("x", 1)],
            &make_instruction(),
            &make_payload(),
            &[],
            &limits,
        );
        assert!(matches!(result, Err(TcbError::InvalidState { .. })));
    }

    // ---- custom envelope: rule-count cap ----

    #[test]
    fn with_limits_smaller_rule_cap_rejects_over() {
        let limits = EnvelopeLimits::new(2, 32, 64, 64).unwrap();
        let core_eval = vec![set_rule("x", 1), set_rule("x", 2), set_rule("x", 3)];
        let result = execute_transition_with_limits(
            &core_eval,
            &make_instruction(),
            &make_payload(),
            &[],
            &limits,
        );
        assert!(matches!(
            result,
            Err(TcbError::TooManyTransformRules {
                limit: 2,
                actual: 3
            })
        ));
    }

    #[test]
    fn with_limits_smaller_rule_cap_accepts_at_boundary() {
        let limits = EnvelopeLimits::new(2, 32, 64, 64).unwrap();
        let core_eval = vec![set_rule("x", 1), set_rule("x", 2)];
        match run_transition(&limits, core_eval) {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x").and_then(|v| v.as_i64()), Some(2));
            }
            other => panic!("expected State, got {other:?}"),
        }
    }

    // ---- custom envelope: execution budget (width defense) ----

    #[test]
    fn with_limits_custom_budget_reports_custom_limit() {
        // budget 16: 1 branch + 17 sub-instructions = 18 > 16; error reports the custom limit
        let limits = EnvelopeLimits::new(1, 16, 64, 64).unwrap();
        let sub_instrs: Vec<JsonValue> = (0..17).map(|i| set_rule("x", i)).collect();
        let core_eval = vec![branch_rule(eq_domain(), sub_instrs)];
        let result = execute_transition_with_limits(
            &core_eval,
            &make_instruction(),
            &make_payload(),
            &[],
            &limits,
        );
        assert!(matches!(
            result,
            Err(TcbError::TooManyExecutedInstructions { limit: 16 })
        ));
    }

    // ---- custom envelope: branch depth ----

    #[test]
    fn with_limits_custom_branch_depth_rejects_over() {
        let limits = EnvelopeLimits::new(1, 64, 2, 64).unwrap();
        let inner = branch_rule(eq_domain(), vec![set_rule("x", 1)]);
        let mid = branch_rule(eq_domain(), vec![inner]);
        let outer = branch_rule(eq_domain(), vec![mid]);
        let result = execute_transition_with_limits(
            &[outer],
            &make_instruction(),
            &make_payload(),
            &[],
            &limits,
        );
        // innermost branch rejected at depth=2 (error reports the custom limit 2)
        assert!(matches!(result, Err(TcbError::NestingTooDeep { limit: 2 })));
    }

    // ---- custom envelope: domain recursion depth ----

    #[test]
    fn with_limits_custom_domain_depth_rejects_over() {
        let limits = EnvelopeLimits::new(1, 16, 64, 2).unwrap();
        // 3 layers of all nesting (leaf eq lands at depth=3 > max 2; over the cap)
        let core_eval = vec![enforce_rule(nested_all_domain(3))];
        let result = execute_transition_with_limits(
            &core_eval,
            &make_instruction(),
            &make_payload(),
            &[],
            &limits,
        );
        assert!(matches!(result, Err(TcbError::NestingTooDeep { limit: 2 })));
    }

    #[test]
    fn with_limits_custom_domain_depth_accepts_at_boundary() {
        let limits = EnvelopeLimits::new(1, 16, 64, 2).unwrap();
        // 2 layers of all nesting (leaf eq lands at depth=2, within the cap): enforce domain true -> Halted
        let core_eval = vec![enforce_rule(nested_all_domain(2))];
        match run_transition(&limits, core_eval) {
            TransitionResult::Halted { .. } => {}
            other => panic!("expected Halted, got {other:?}"),
        }
    }

    // ---- default equivalence: default entry and with_limits(Default) behave identically ----

    #[test]
    fn default_entry_matches_with_limits_default() {
        let core_eval = vec![set_rule("x", 7), set_rule("y", 8)];
        let instruction = make_instruction();
        let payload = make_payload();

        let a = execute_transition(&core_eval, &instruction, &payload, &[]);
        let b = execute_transition_with_limits(
            &core_eval,
            &instruction,
            &payload,
            &[],
            &EnvelopeLimits::default(),
        );
        assert_eq!(a.is_ok(), b.is_ok());
        if let (
            Ok(TransitionResult::State {
                new_payload: pa, ..
            }),
            Ok(TransitionResult::State {
                new_payload: pb, ..
            }),
        ) = (a, b)
        {
            assert_eq!(pa, pb);
        }
    }
}
