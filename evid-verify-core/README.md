# evid-verify-core

The EvoRule EVID standalone verification kernel: a `no_std` + `alloc` crate performing **zero-trust integrity verdicts** over audit-chain data — given an export document / WAL row series / anchor seal chain, it reaches a verdict by recomputing from the formulas, trusting no external conclusion.

## Three verification surfaces (report-style, stop at the first red)

| Surface | Entry | Steps |
|---|---|---|
| Export surface | `verify::verify_export` | Four steps per entry: prev linkage → content hash (`fact_hash` formula recomputation) → stable deep compare (`fact_to_stable_json` recomputation reconciled against the entry's original text) → chain folding; the chain tail is reconciled against `last_hash` |
| WAL surface | `verify::verify_wal_lines` | Three steps per row: prev linkage → `content_hash` recomputation → `chain_hash` chain folding |
| Anchor surface | `seal::verify_seal` / `verify_seal_chain` | Four rules: seq continuity → gapless intervals → `anchor_hash` chain → ed25519 signature verification (`evorule-anchor/2` payload semantics) |

After a chain break, the folded chain head loses its anchor; subsequent entries are faithfully marked undetermined (`stopped_at`); all green = no stop point and the chain tail reconciles successfully against the declared tail hash.

## Formula discipline (SSOT)

- `fact_hash = blake3(to_string(fact_to_stable_json(fact)))`
- `chain_step = blake3(prev_hex ++ content_hex)`, chain head prev = `genesis`
- `anchor_hash = blake3(prev_anchor ++ '|' ++ the nine payload fields)`, anchor chain head prev = `anchor-genesis`

All fact-chain formulas are invoked through the `evorule-audit-chain` SSOT; this crate never hand-writes a fact-chain hash path; direct blake3 calls are only for anchor payload hashing (field-aligned with the server-side anchor.rs, guarded by the vector gate).

## fail-closed shape checks

- The export version accepts only `1.0`; anchors accept only v2 payload semantics (the 0.8.1 v1 anchors are incompatible; mixed-version chains are judged as failures, never silently cross-format verified);
- Unknown Fact variants / missing fields / type mismatches / floating-point fields are rejected outright;
- The numeric channels are dual with SSOT serialization: full i64 range → `Integer`, high u64 → `U64`.

## Usage

```rust
use evid_verify_core::record::parse_export_doc;
use evid_verify_core::verify::verify_export;

let v: serde_json::Value = serde_json::from_str(&input_json)?;
let doc = parse_export_doc(&v)?;   // fail-closed parsing (ParseError is a structured red)
let report = verify_export(&doc);  // per-entry verdict report
```

The `g5` golden-vector self-test (11 vectors) is embedded (verified by `cargo test`), covering the parse → stable-recompute roundtrip and the three verification surfaces.

## Honest boundaries

- This kernel judges "the chain has not been tampered with", not "the chain head records the truth" — that is the domain of signing anchors and threshold signatures;
- JSON numbers carry no form tags: `U64(5)` settles back to `Integer(5)` after a parse roundtrip (value fidelity preserved; the two forms are stable-byte-equivalent and do not affect hash verdicts);
- Anchor public keys are injected by the caller (looked up by `key_id`); this crate contains no key generation / clock / randomness.

## Downstream

- [`evid-verify-wasm`](https://github.com/evorule/evorule/tree/main/evid-verify-wasm): the browser install-free single-file verifier and the wasm-pack/npm binding surface;
- Third-party Rust hosts (mobile apps, server re-checks, ops sentinels) depend on this crate directly to verify chains offline.
