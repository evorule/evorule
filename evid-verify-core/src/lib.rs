// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EVID mobile audit-chain independent verification kernel (no_std)
//!
//! Zero-trust verdict kernel: given audit-chain data (export doc / WAL line
//! series / anchor seal chain), it completes integrity verdicts by recomputing
//! from the formulas and trusts no external conclusions. Shared by three
//! consumers: the wasm binding (mobile single-file HTML verifier), server-side
//! re-checks, and sentinel cross-checks.
//!
//! # Three verdict surfaces
//!
//! - **Export surface** ([`verify::verify_export`], four steps per entry):
//!   prev link → content hash (`fact_hash` formula recomputation) → stable
//!   deep-compare (`fact_to_stable_json` recomputation reconciled with the
//!   `fact_json_stable` original + entry projection consistency) → chain
//!   folding; the tail reconciles with `last_hash`.
//! - **WAL surface** ([`verify::verify_wal_lines`], three steps per line):
//!   prev link → `content_hash` recomputation → `chain_hash` chain folding.
//! - **Anchor surface** ([`seal::verify_seal_chain`], four rules): contiguous
//!   seq → seamless ranges → anchor_hash chain → ed25519 signature
//!   verification (v2 payload semantics).
//!
//! # Formula discipline
//!
//! `fact_hash = blake3(to_string(fact_to_stable_json(fact)))`;
//! `chain_step = blake3(prev_hex ++ content_hex)`, chain-head prev =
//! `genesis`; `anchor_hash = blake3(prev_anchor ++ '|' ++ nine payload
//! fields)`, anchor chain-head prev = `anchor-genesis`. All fact-chain
//! formulas go through the [`evorule_audit_chain::hash`] SSOT; this crate
//! never hand-writes a fact-chain hash path.
//!
//! # fail-closed shapes
//!
//! - The export version must be `1.0`; anchors accept only v2 payload
//!   semantics (`evorule-anchor/2`; v1 anchors from 0.8.1 are incompatible —
//!   stated as an honest boundary);
//! - Unknown Fact variants / missing fields / type mismatches / float fields
//!   are all rejected;
//! - The number channel is dual to the SSOT serialization: full i64 range →
//!   `Integer`, u64 high bits → `U64`, floats rejected (full-u64 discipline).
//!
//! # Honest boundary
//!
//! - This kernel judges "the chain was not tampered with", not "the chain
//!   head records the truth" (that is the domain of signed anchors and T3
//!   threshold signatures);
//! - JSON numbers carry no form tag: after a parse roundtrip `U64(5)` lands
//!   on `Integer(5)` (value fidelity; both forms produce equivalent stable
//!   bytes, so hash verdicts are unaffected).
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod parse;
pub mod record;
pub mod seal;
pub mod verify;
