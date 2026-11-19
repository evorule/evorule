<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

<a id="english"></a>

# ADR-0002: Carrier for Model Weakening in Tier-B Kani Proofs — from impl-level cfg Dual Implementation to Proof-level Stubs

- **Status**: Accepted
- **Date**: 2026-09-13
- **Deciders**: Repository maintainer (see [AUTHORS.md](../../AUTHORS.md))
- **Informed**: —

## Background and Problem

The 23 Tier-B TCB Kani proofs (covering P0 properties P0-1/2/4/5/7/8) all timed out at 600s/3600s in practice ([STATUS.md](../../verification/STATUS.md) Appendix B, measured 2026-09-11). After the two earlier remediation efforts (KaniMap storage backend, bounded recursion depth) resolved the first two root-cause layers, the third layer remained: symbolic container encoding explosion of `==`/`clone` inside the eq branch (round-9 three-variant probe, recorded in the probe rounds log). The original carrier plan carried this modeling via impl-level `cfg(kani)` dual implementation: container variants compare length only for `==` and clone to empty containers, while scalar variants keep real behavior. Carrier review found two structural defects in the impl-level override: ① **uncontrollable blast radius** — the override affects every call site in kani builds, including harness construction (e.g. `v.clone()` inside `object_from_pairs`), silently collapsing nested composite inputs at depth 1 so proofs verify degraded inputs; ② **broken compensation** — the real trait impl disappears from the kani build artifact, so any compensation registered as "independent symbolic coverage of the real implementation" (e.g. the eq model compensated by the Tier-A `verify_partial_eq_never_panics` side proof) references the overridden model itself, a circular reference. A carrier decision was required.

## Decision Drivers

- **Soundness**: never-panic proofs must not miss genuinely reachable branches (construction-layer degradation) nor lose the basis of compensation (circular reference) due to the carrier choice
- **Auditability**: the existence and scope of model weakening must be visible to proof readers (declaration as disclosure), not buried in src
- **Minimal blast radius**: modeling should affect only the harness that declares it, not other proofs reusing JsonValue (e.g. reactor)
- **Governability**: "weakening + side-proof compensation" is this project's standard deviation-governance pattern; the carrier must keep that pattern viable

## Options Considered

1. **Option A**: impl-level `cfg(kani)` dual implementation (original carrier plan)
2. **Option B**: proof-level `#[kani::stub(...)]` (native Kani stubbing, `-Z stubbing`), no impl-level override in value.rs
3. **Option C**: no modeling; route all non-runnable Tier-B properties to TLA+ / type-system gates / test fallbacks

## Decision Outcome

**Option B was chosen** (with Option C's property routing as a parallel measure), because stub declarations sit on the proof header making the weakening explicit and auditable, the blast radius is limited to a single harness, and the real PartialEq/Clone always remain in the kani build artifact — proofs without stubs (e.g. Tier-A `verify_partial_eq_never_panics`) keep verifying the real implementation, so the "model weakening + real-implementation side-proof compensation" governance pattern is structurally sound. Option A's two defects are described in the background; Option C abandons Kani coverage of end-to-end orchestration logic — too costly as a global strategy, though routing is retained for properties it naturally fits (structural induction, overflow safety).

### Consequences

- Upside: weakening made explicit (visible on proof headers); zero impact on reactor gate proofs; compensation chain viable; governance rules become machine-checkable once the cfg(kani) deviation register (K-series) gains a "blast radius" column
- Downside: depends on the unstable `-Z stubbing` feature (Kani 0.67.0, version pinned); sound-but-incomplete stubs may yield false-positive counterexamples (manual triage: real defect vs. over-approximate stub); CI commands need an extra flag
- How the negative effects are mitigated: stubbing availability is verified in Phase 0 (if unavailable, controlled fallback to Option A with compensation downgraded to test-level and explicitly registered under the register rule "compensation must not reference proofs affected by the model"); stub models follow the over-approximation principle (the eq model uses a nondeterministic boolean, covering every possible outcome of real eq)

### Validation

- Metrics: all 14 Tier-A proofs re-run green after the carrier rollback (regression confirmation that real impls are restored); stub pilot proofs (one from the eq family, one from the meta-instruction family) converge within 600s each; register entries K-4/K-5 pass the "compensation must not reference the affected dispatch set" check
- Review checkpoint: Phase 0 feasibility matrix (item T0-2); after the Phase 2 stub pilots
- Conditions that trigger a rollback: `-Z stubbing` unavailable on the pinned Kani version (fallback to Option A with compensation downgrade registered); stub pilots still exceeding 600s (that property family moves to TLA+/downgrade — the carrier decision itself stands)

## Pros and Cons of Each Option

### Option A

- Pros: no unstable-flag dependency; implementation draft already existed in the original carrier plan
- Cons: blast radius covers all kani-build call sites (including construction — inputs silently degrade); real impl absent from the build artifact, compensation circular; every change requires re-justifying reactor impact

### Option B

- Pros: weakening explicit and auditable; blast radius limited to declaring harnesses; real impl retained in the build, compensation viable; zero reactor impact
- Cons: `-Z stubbing` dependency (unstable, mitigated by version pinning); false-positive triage cost; CI configuration change

### Option C

- Pros: zero Kani-mechanism dependency; each property uses its naturally matching tool (structural induction → TLA+, overflow → type-system gates)
- Cons: abandons Kani's symbolic coverage of end-to-end orchestration (execute_transition family); the 23 existing Tier-B proofs sit idle

## More Information

- Original carrier plan with rounds 1-9 probe records: see the project's internal change archive (not published in this repository)
- Implementation change for this decision (stub rollout): see the project's internal change archive (not published in this repository)
- Tier-B timeout measurements and tiers: [verification/STATUS.md](../../verification/STATUS.md) Appendix B
- Disclosure entry: [verification/DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md), 2026-09-13 entry
