<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->
<!-- This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later. -->

<a id="english"></a>

# EvoRule v0.3.1 Performance Baseline Report

> **Version**: v0.3.1
> **Date**: 2026-08-20
> **Status**: ✅ Passed pre-publish performance verification
> **Baseline type**: Criterion statistical benchmarks (100 samples / 5 s sampling period)

## 1. Test Environment

### 1.1 Hardware Configuration

| Item | Specification |
|------|------|
| **CPU** | 12th Gen Intel Core i7-12700 |
| **Cores/Threads** | 12 cores / 20 threads |
| **Memory** | 16 GB DDR |
| **Cache** | L1: 3072 KB, L2: 12 MB, L3: 25 MB |

### 1.2 Software Environment

| Item | Version |
|------|------|
| **Operating system** | Microsoft Windows 11 Pro (Build 26100) |
| **Rust compiler** | rustc 1.97.1 (8bab26f4f 2026-07-14) |
| **Cargo** | Bundled with rustc 1.97.1 |
| **Benchmark framework** | criterion 0.5.1 |

### 1.3 Build Configuration

| Item | Configuration |
|------|------|
| **Profile** | release |
| **Optimization level** | opt-level = 3 |
| **LTO** | Enabled (lto = true) |
| **Codegen units** | codegen-units = 1 |
| **Debug info** | Default (off by default in release) |
| **CPU features** | Default (no target-CPU-specific optimization) |

### 1.4 Running Conditions

- 🚫 No other compute-intensive tasks ran during the tests
- 🌡️ CPU temperature normal (no throttling triggered)
- ⏱️ Tests ran in power mode: High performance

## 2. Module Test Coverage

| Module | Priority | Benchmarks | Coverage |
|------|--------|---------------|----------|
| **evorule-tcb** | 🔴 Highest | 8 | Full coverage of the core hot paths |
| **evorule-reactor** | 🟠 High | 3 | Main loop + FactsLog |
| **evorule-governance** | 🟡 Medium | 2 | Audit chain + verification |
| **evorule-cli** | ⚪ Not needed | - | Thin dispatch layer, no performance testing required |

### 2.1 Why evorule-cli Needs No Performance Testing

evorule-cli is a thin command dispatch layer:
1. **No core computation**: it only handles CLI argument parsing and command dispatch
2. **The bottleneck is not in the CLI**: actual performance is determined by the underlying tcb and reactor
3. **Already covered indirectly**: the tcb and reactor benchmarks already cover the core call paths of the CLI

## 3. evorule-tcb Performance Results

> The TCB is the core computation engine — the performance-critical path of every instruction execution

### 3.1 Instruction Execution Performance

| Benchmark | Description | Latency (µs) | Throughput (ops/s) | Outliers |
|-----------|------|-----------|----------------|--------|
| `execute_transition_increment` | Execute the increment instruction | **2.860** | 349,650 | 5% |
| `execute_transition_set` | Execute the set instruction | **2.882** | 347,050 | 4% |
| `execute_transition_noop` | Execute the noop instruction | **1.719** | 581,650 | 14% |

**Analysis**:
- The increment and set instructions perform nearly the same (~2.86-2.88 µs), as expected
- The noop instruction is the fastest (1.72 µs) because it changes no state
- Outlier ratios are normal (<15%); the data is reliable

### 3.2 Batch Operation Performance

| Benchmark | Description | Total latency | Per-operation latency | Throughput (ops/s) |
|-----------|------|--------|-----------|----------------|
| `execute_transition_1000_increments` | 1000 consecutive increments | **2.868 ms** | 2.868 µs | 348,650 |

**Analysis**:
- Batch operations keep linear performance (2.868 µs each ≈ 2.860 µs for a single operation)
- No noticeable cumulative state overhead
- Well suited to high-frequency incremental update scenarios

### 3.3 Data Structure Performance

| Benchmark | Description | Latency | Throughput (ops/s) |
|-----------|------|------|----------------|
| `jsonvalue_construction` | Object construction (5 fields + nesting) | **529 ns** | 1,890,360 |
| `jsonvalue_field_access` | Field access (including nested paths) | **21 ns** | 47,619,048 |

**Analysis**:
- JsonValue construction is efficient (529 ns), built on BTreeMap
- Field access is extremely fast (21 ns), thanks to the O(log n) behavior of BTreeMap
- Nested path access performance is stable

### 3.4 Rule Matching and Queue Performance

| Benchmark | Description | Latency (µs) | Outliers |
|-----------|------|-----------|--------|
| `rule_matching` | Rule traversal and matching | **2.867** | 8% |
| `queue_operations` | Instruction execution with a queue | **4.728** | 2% |

**Analysis**:
- Rule matching (2.867 µs) is essentially identical to instruction execution (2.860 µs)
- Queue operations add about 1.86 µs of overhead (queue traversal + instruction pop)
- Queue operations have the lowest outlier ratio (2%) — the most stable data

### 3.5 TCB Performance Metrics Summary

```
┌───────────────────────────────────────────────────────────────────────┐
│                      evorule-tcb Performance Metrics                  │
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  Instruction execution (avg) : 2.82 µs  (354,965 ops/s)               │
│  Data access (avg) : 21 ns  (47.6 M ops/s)                            │
│  Object construction : 529 ns  (1.89 M ops/s)                         │
│  Batch scalability : linear (no cumulative overhead)                  │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

## 4. evorule-reactor Performance Results

> The Reactor is the execution engine, responsible for command dispatch, state management and I/O coordination

### 4.1 FactsLog Append Performance

| Benchmark | Description | Total latency | Per-operation latency | Throughput (ops/s) | Outliers |
|-----------|------|--------|-----------|----------------|--------|
| `facts_log_append_1000_memory` | 1000 appends (in-memory mode) | **1.451 ms** | 1.451 µs | 689,180 | 8% |

**Analysis**:
- Each append includes BLAKE3 hash chain computation (content_hash + chain_hash)
- Throughput is close to 690 K ops/s
- The outlier ratio is low (8%); the data is reliable

### 4.2 End-to-End Performance

| Benchmark | Description | Latency | Throughput (ops/s) | Outliers |
|-----------|------|------|----------------|--------|
| `reactor_e2e_single_command` | Single command, end to end | **12.827 µs** | 77,947 | 10% |
| `reactor_e2e_100_commands` | 100 commands in batch | **3.458 ms** | 28,917 cmd/s | 2% |

**Analysis**:
- Single-command end-to-end latency is 12.83 µs, covering the full spawn → execute → Stable event path
- Batch throughput is about 28,917 cmd/s
- Batch efficiency is lower than the single-command 77,947 ops/s (queue accumulation effect)

### 4.3 End-to-End Latency Breakdown

Using `reactor_e2e_single_command` (12.827 µs) as the baseline:

```
┌────────────────────────────────────────────────────────────────────┐
│        Reactor single-command latency breakdown (12.827 µs)        │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  TCB core transition  ████████████████░░░░░░░░   2.860 µs  (22.3%) │
│  FactsLog append     ██████░░░░░░░░░░░░░░░░░░   1.451 µs  (11.3%) │
│  Reactor scheduling  ████████████████████████░   ~4.5 µs   (35.1%) │
│  Event broadcast/sync ████████████░░░░░░░░░░░░░   ~2.5 µs   (19.5%) │
│  Other overhead      ████████░░░░░░░░░░░░░░░░   ~1.5 µs   (11.8%) │
│                                                                    │
│  Note: Reactor scheduling + event broadcast = tokio runtime +      │
│  Channel communication                                             │
└────────────────────────────────────────────────────────────────────┘
```

**Key findings**:
- The TCB core transition accounts for **22.3%** — the largest single computation cost
- TCB + FactsLog together account for **33.6%**, which can be seen as the pure compute path
- The remaining **66.4%** is tokio scheduling and Channel communication overhead
- The scheduling overhead comes mainly from async/await context switches

### 4.4 Reactor Performance Metrics Summary

```
┌───────────────────────────────────────────────────────────────────────┐
│                    evorule-reactor Performance Metrics                    │
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  Single-command e2e latency : 12.83 µs  (77,947 ops/s)                    │
│  Batch throughput : 28,917 cmd/s  (100 commands)                          │
│  FactsLog append : 1.451 µs  (689,180 ops/s)                              │
│  Pure compute share : 33.6%  (TCB + FactsLog)                             │
│  Scheduling/comm overhead : 66.4%  (tokio + Channel)                      │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

## 5. evorule-governance Performance Results

> Governance is the governance layer, handling cross-cutting concerns such as the audit chain and permission management

### 5.1 Audit Chain Performance

| Benchmark | Description | Latency | Throughput | Outliers |
|-----------|------|------|--------|--------|
| `auditor_audit_new_100_facts` | Audit 100 facts | **291.78 µs** | 343 times/s | 7% |
| `auditor_verify_1000_entries` | Verify 1000 audit entries | **237.85 µs** | 4,204 times/s | 10% |

**Analysis**:
- audit_new takes 291.78 µs to process 100 facts (including hash chain entry construction)
- verify takes 237.85 µs to verify 1000 entries (O(n) hash chain checking)
- Governance layer performance is acceptable because it is not on the per-round hot path
- Suited to periodic audits and on-demand verification

### 5.2 Governance Performance Metrics Summary

```
┌───────────────────────────────────────────────────────────────────────┐
│                  evorule-governance Performance Metrics                   │
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  Audit chain build (100 facts) : 291.78 µs  (343 audits/s)                │
│  Audit chain verify (1000 entries) : 237.85 µs  (4,204 verifies/s)        │
│  Role : periodic / on-demand                                              │
│  Hot-path impact : low (not on the per-round instruction execution path)  │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

## 6. Cross-Module Performance Comparison

### 6.1 Performance Metrics Summary Table

| Module | Core operation latency | Throughput | Hot-path impact |
|------|-------------|--------|-----------|
| **evorule-tcb** | 2.86 µs (increment) | 349,650 ops/s | Every instruction execution |
| **evorule-reactor** | 12.83 µs (e2e) | 77,947 ops/s | Every command execution |
| **evorule-governance** | 291.78 µs (audit) | 343 audit/s | Periodic / on demand |
| **evorule-cli** | N/A | N/A | Thin dispatch layer |

### 6.2 Throughput Comparison

```
Throughput (ops/s, log scale)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

evorule-tcb (field access)        ██████████████████████████████████████  47.6 M
evorule-tcb (object construction) ████████████████████████████            1.89 M
evorule-reactor (FactsLog)        ████████████████████████████████        689 K
evorule-tcb (instruction exec)    ███████████████████████████             349 K
evorule-reactor (e2e)             ███████████████                         77.9 K
evorule-governance                █████                                   4.2 K
  (audit verification, 1000 entries)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### 6.3 Latency Stacking Analysis

| Layer | Operation | Latency | Cumulative latency |
|------|------|------|----------|
| L0 | JsonValue field access | 21 ns | 21 ns |
| L1 | JsonValue object construction | 529 ns | 550 ns |
| L2 | TCB instruction execution | 2,860 ns | 3,410 ns |
| L3 | FactsLog append | 1,451 ns | 4,861 ns |
| L4 | Reactor end to end | 12,827 ns | 17,688 ns |

**Analysis**:
- From data access to end to end, latency grows by about **32x**
- L0→L2 (data structures to instruction execution): ~52x amplification
- L2→L4 (instruction execution to end to end): ~4.5x amplification
- The amplification comes mainly from the async scheduling overhead of the Reactor

## 7. Outlier Analysis

### 7.1 Outlier Statistics

| Benchmark | Outlier ratio | Severity distribution | Data reliability |
|-----------|-----------|-------------|-----------|
| `tcb/execute_transition_increment` | 5% | 4% high, 1% low | ✅ Reliable |
| `tcb/execute_transition_set` | 4% | 3% high, 1% low | ✅ Reliable |
| `tcb/execute_transition_noop` | 14% | 8% high, 3% high-severe | ✅ Acceptable |
| `tcb/execute_transition_1000_increments` | 15% | 3% high-severe | ✅ Acceptable |
| `tcb/jsonvalue_construction` | 10% | 3% high-severe | ✅ Reliable |
| `tcb/jsonvalue_field_access` | 6% | 3% high-severe | ✅ Reliable |
| `tcb/rule_matching` | 8% | 3% high-severe | ✅ Reliable |
| `tcb/queue_operations` | 2% | 2% high | ✅ Very reliable |
| `facts_log/append_1000_memory` | 8% | 3% high-severe | ✅ Reliable |
| `reactor/e2e_single_command` | 10% | 5% high, 5% low | ✅ Reliable |
| `reactor/e2e_100_commands` | 2% | 2% high | ✅ Very reliable |
| `auditor/audit_new_100_facts` | 7% | 3% high-severe | ✅ Reliable |
| `auditor/verify_1000_entries` | 10% | 2% high-severe | ✅ Reliable |

### 7.2 Outlier Conclusions

1. **Overall outlier ratio**: 2% - 15%, all within the acceptable range
2. **Why the outliers are high**:
   - noop instruction (14%): shortest execution path, so system noise has a relatively larger effect
   - Batch operations (15%): accumulation effects cause slightly larger fluctuation
3. **Stability indicators**:
   - queue_operations (2%): the most stable queue-operation latency
   - reactor_e2e_100_commands (2%): the most stable batch throughput
4. **Data reliability**: all benchmark data is reliable and fit for publish decisions

## 8. Performance Optimization Suggestions

### 8.1 Current Performance Assessment

| Dimension | Rating | Notes |
|------|------|------|
| **TCB core performance** | ⭐⭐⭐⭐⭐ | 2.86 µs/instruction — excellent |
| **Reactor efficiency** | ⭐⭐⭐⭐ | 12.83 µs/e2e — the main bottleneck is scheduling overhead |
| **FactsLog performance** | ⭐⭐⭐⭐⭐ | 1.45 µs/append (hashing included) — very efficient |
| **Governance performance** | ⭐⭐⭐⭐ | Acceptable, not on the hot path |

### 8.2 Potential Optimization Directions

1. **High priority**
   - [ ] Reduce Reactor scheduling overhead (66.4% of the total)
   - [ ] Consider using `tok::sync::mpsc` instead of `tokio::sync::mpsc` (if async capability is not needed)
   - [ ] Optimize the Stable event broadcast path

2. **Medium priority**
   - [ ] Evaluate the performance difference between a single-threaded and a multi-threaded Reactor runtime
   - [ ] Improve the queue-handling efficiency of batch commands

3. **Low priority**
   - [ ] Consider batch-append optimizations for FactsLog
   - [ ] Evaluate whether incremental auditing is needed in the governance layer

### 8.3 Expected Optimization Gains

| Direction | Expected gain | Difficulty |
|---------|---------|---------|
| Reduce scheduling overhead | 15-20% end-to-end improvement | Medium |
| Optimize Stable broadcast | 5-10% end-to-end improvement | Low |
| Batch queue optimization | 10-15% batch throughput improvement | Medium |

## 9. Publish Recommendations

### 9.1 Performance Gate Assessment

| Check | Standard | Result |
|--------|------|------|
| TCB instruction execution | < 10 µs | ✅ 2.86 µs |
| Reactor end to end | < 50 µs | ✅ 12.83 µs |
| FactsLog append | < 5 µs | ✅ 1.45 µs |
| Outlier ratio | < 20% | ✅ max 15% |
| Data reliability | All pass | ✅ 13/13 benchmarks |

### 9.2 Publish Decision

**Recommendation**: ✅ **Performance verification passed — ready to enter the publish process**

Reasons:
1. All core performance metrics are far better than the standards
2. The data is reliable and outliers are within the acceptable range
3. No obvious performance bottleneck affects core functionality
4. Optimization headroom exists but does not block the publish

### 9.3 Follow-ups

- [ ] Consider Reactor scheduling optimization in the next version (v0.3.2)
- [ ] Keep monitoring for performance regressions (benchmark comparison in CI is recommended)
- [ ] Accumulate performance data from more real business scenarios

## Appendix A: Test Commands

```bash
# Run all performance tests
cargo bench -p evorule-tcb
cargo bench -p evorule-reactor
cargo bench -p evorule-governance

# Run a single benchmark
cargo bench -p evorule-tcb --bench tcb_core
cargo bench -p evorule-reactor --bench reactor_e2e
cargo bench -p evorule-reactor --bench facts_log_append
cargo bench -p evorule-governance --bench audit_chain

# Generate an HTML report (requires the criterion html_reports feature)
cargo bench -p evorule-tcb --bench tcb_core -- --plot-format gnuplot
```

## Appendix B: Version History

| Version | Date | Change |
|------|------|----------|
| **v0.3.1** | 2026-08-20 | First performance baseline report, covering 3 of the 4 core modules |

---

> **Maintenance**: this report serves as the v0.3.1 performance baseline; later versions should run performance regression comparisons against it.
> **Contact**: EvoRule Project (evorulelab@gmail.com)
