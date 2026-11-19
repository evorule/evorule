<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# How to Verify Fact Chain Hash Integrity

> Task: confirm that a fact log has not been tampered with, and that every execution step carries BLAKE3 hash evidence plus structural invariant guarantees.

## Why verification matters

The core promise of evorule is "execution is provable". Every state transition generates hashes, and consecutive hashes form a chain. Verification proves that:

1. the fact log content has not been tampered with
2. the execution order has not been rearranged (FactId monotonically increasing)
3. cause references point to valid preceding facts
4. the hash chain has no breaks

## Steps

```bash
evorule verify-chain fact-log.jsonl
```

Replace `fact-log.jsonl` with the path to your fact log file.

Code basis: `evorule-cli/src/commands/verify_chain.rs` L34-70.

## Three-layer verification

`verify-chain` performs three layers of verification (verify_chain.rs L6-9):

### 1. Hash chain verification (new WAL format)

Checks the three hash fields of each record in turn (verify_chain.rs L80-137):

| Field | Verification logic |
|------|---------|
| `content_hash` | Recomputes `fact_hash(fact)` and compares it with the stored value |
| `prev_hash` | The stored value must equal the previous record's `chain_hash` (`"genesis"` for the first record) |
| `chain_hash` | Recomputes `blake3(prev_hash + content_hash)` and compares it with the stored value |

### 2. FactId monotonically increasing

Each Fact's `id` must be strictly greater than the previous one (verify_chain.rs L175-183). Detects tampered or reordered ids.

### 3. cause reference validity

`StateTransition.cause` and `IoRequest.cause` must point to a FactId that has already appeared (verify_chain.rs L185-198). Detects dangling references.

## Supported WAL formats

| Format | Hash verification | Structural verification | Detection |
|------|---------|---------|---------|
| New WAL format (with `content_hash`/`prev_hash`/`chain_hash`) | ✅ Full | ✅ | `read_wal_with_hash` succeeds and hash fields are present |
| Old WAL format (with `version_before`/`fact`, no hash fields) | ❌ | ✅ | `read_wal_with_hash` succeeds but no hash fields |
| CLI raw format (one Fact JSON per line) | ❌ | ✅ | `read_wal_with_hash` fails, falls back to `read_facts` |

The old format and the CLI raw format emit a `[WARN]` noting that only structural checks were performed (verify_chain.rs L55, L65).

## Reading the output

**All checks pass**:
```
=== Verifying hash chain: fact-log.jsonl ===
Algorithm: blake3 (unified with evorule-reactor WAL)

Facts: 5 (tier1 WAL format)
[INFO] New WAL format detected (with hash fields)
[OK] Hash chain verified (content_hash + prev_hash + chain_hash)
[OK] Structural invariants verified (FactId monotonic, cause references valid)
     genesis → F1 → F2 → ... → F5 (final)
```

**Verification failure**: the output pinpoints the error, including:
- the break location (which Fact, its Fact ID)
- the error type (content_hash mismatch / prev_hash mismatch / chain_hash mismatch / monotonicity violated / dangling cause)
- the stored value vs the recomputed value

## Exit codes

| Exit code | Meaning | Code basis |
|--------|------|---------|
| 0 | Hash chain + structural invariants all pass | verify_chain.rs L32 |
| 1 | Any check fails | verify_chain.rs L33 |

## Hash algorithm

```
content_hash = fact_hash(fact)           // BLAKE3 hash of the Fact content
chain_hash   = blake3(prev_hash + content_hash)
```

- The first record has `prev_hash = "genesis"`
- The algorithm SSOT lives in `evorule-reactor/src/hash.rs`
- The `test_cross_validate_with_tier2` test guarantees reactor/governance/cli consistency

## Difference from audit anchors

| Mechanism | Purpose | Protects against | Command |
|------|------|--------|------|
| `verify-chain` | Verifies the internal hash chain + structural integrity of a fact log | Tampering/reordering/deletion/dangling references | `evorule verify-chain` |
| `verify-anchors` | Verifies the digital signatures of an audit export | Repudiation/forged origin | `evorule verify-anchors` |

`verify-chain` proves "this log is self-consistent"; `verify-anchors` proves "this log was indeed signed by the holder of the specified private seed". The two complement each other.

## FAQ

**Q: Does a passing verify-chain guarantee safety?**
A: verify-chain proves the log is internally consistent and structurally complete; it does not prove that the log's origin is trustworthy. For non-repudiation, combine it with `verify-anchors` audit anchor signatures.

**Q: What does the [WARN] Old WAL format output mean?**
A: Your fact log is in the old format (no hash fields), so only structural checks are possible (FactId monotonicity + cause references); the hash chain cannot be verified. Regenerate the fact log with a current version.

**Q: Can I verify only part of a log?**
A: `verify-chain` currently verifies the complete chain. To verify a subset, cut out a contiguous segment and verify it (the first segment's prev_hash must be confirmed manually).

## Related commands

- [`evorule run`](./execute-rules.md) — generate a fact log
- [`evorule replay`](./replay-fact-log.md) — view fact log contents
- Audit anchors: `evorule anchor-keygen` + `evorule verify-anchors` (see [Audit anchor guide](./audit-anchors.md))
