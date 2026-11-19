# evorule-audit-chain

The `EvoRule` audit-chain no_std kernel — the **single source of truth** for Fact data shapes + stable JSON serialization + BLAKE3 chain hashing.

- `#![no_std]` + alloc: one implementation serves both std hosts (evorule-reactor / governance / cli, consumed via re-export) and embedded targets (evorule-mcu).
- Determinism: same input, same output; serialization key order fixed by BTreeMap; the `hash_snapshot.txt` snapshot test guards that existing hashes never change.
- Lift-out record: 2026-10-18 (migrated from evorule-reactor, `std::sync::Arc` → `alloc::sync::Arc`, behavior verbatim identical).

## Hash chain algorithm

```text
prev_hash initial = "genesis"
content_hash = blake3(fact_to_stable_json(fact))   // explicit JSON, stable format
chain_hash   = blake3(prev_hash + content_hash)
```

## License

AGPL-3.0-or-later
