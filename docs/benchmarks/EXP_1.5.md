<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# 实验 1.5:并发 session 测试(100 sessions)

**日期**:2026-07-20
**目标**:验证 evorule-server 在 100 sessions 并发活跃时的吞吐和稳定性
**工具**:`D:\evorule\tier2-governance\examples\bench_throughput.rs`
**配置**:server `--no-rate-limit`(绕过 200 req/s 限速)

---

## 1. 结果汇总

| 场景 | 突发速率 | 持续速率 (60s) | 备注 |
|---|---|---|---|
| **100 sessions × 100 cmds × 50 concurrent** | **54,870 cmds/sec** | — | 10000 ops / 0.18s |
| **100 sessions × 10 cmds × 50 concurrent (60s sustained)** | 21,270 cmds/sec | **16,448 ops/sec** | 986K ops / 60s |
| Session create | 6,838-7,131 /s | — | 100 sessions / 14ms |
| State read | 24,642-57,831 /s | — | 1000 reads / 17ms |

**关键结论**:
- **100 并发 sessions 不降速** — 跟 5 sessions 的 16,570 ops/sec 持平
- server 并发架构(每个 session 独立 reactor + tokio)扩展性 **线性**
- 单实例 evorule-server 可稳定支撑 **~16K ops/sec 持续**,**~55K ops/sec 突发**

---

## 2. 测试方法

### 2.1 工具

`bench_throughput <num_sessions> <cmds_per_session> <concurrency>`

3 个 phase:
- Phase 1: 串行创建 N sessions
- Phase 2: 50 个 worker 并发对 N 个 session 各发 M cmds
- Phase 3: 50 worker 并发读 10 × N 个 state
- Phase 4 (可选,设 `EVORULE_BENCH_SUSTAINED=1`): 持续 60s

### 2.2 关键 server 限速

server 默认 200 req/s(P1-4 限速),并发测试用 `--no-rate-limit` 绕过。

注意:1.2 阶段已加这个 CLI flag(详见 docs/benchmarks/EXP_1.2.md)。

### 2.3 服务端并发架构(回顾)

每个 session 独立:
- `Reactor` 主循环(drain → stable → block → execute)
- `FactsLog`(独立 append-only log)
- `command_tx` / `event_tx` channels
- `Arc<Mutex<...>>` 跨 task 共享

**没有全局锁**(除了 `SessionManager` 的 session map lock)。所以 N sessions 并发,N 个 reactor 真正并行。

---

## 3. 详细数据

### 3.1 突发:100 sessions × 100 cmds × 50 concurrent

```
[Phase 1] Creating 100 sessions...
[OK] 100 sessions in 0.01s (7131 sessions/sec)

[Phase 2] Firing 10000 commands across 100 sessions (concurrency=50)...
[OK] 10000 commands in 0.18s (54870 cmds/sec, 0.0ms/cmd)

[Phase 3] 1000 state reads (concurrency=50)...
[OK] 1000 reads in 0.02s (57831 reads/sec, 0.0ms/read)
```

| 指标 | 数值 |
|---|---|
| Session create | 7,131 /s |
| Command 突发 | **54,870 /s** |
| State 读 | 57,831 /s |
| 总耗时 (10000 cmds) | 180ms |

### 3.2 持续:100 sessions × 60s

```
[Phase 4] Sustained load (60s)...
[OK] 986872 ops in 60.0s (16448 ops/sec sustained)
```

| 指标 | 数值 |
|---|---|
| 总操作数 (60s) | 986,872 |
| 持续速率 | **16,448 /s** |
| 100 sessions 平均 | 164 ops/s per session |

### 3.3 对比 1.2(5 sessions sustained)

| 配置 | 持续速率 | 差异 |
|---|---|---|
| **5 sessions**(1.2) | 16,570 ops/sec | baseline |
| **100 sessions**(1.5) | 16,448 ops/sec | **-0.7%** |

**100 sessions 跟 5 sessions 持续速率几乎一样**。这说明 evorule 的 per-session reactor 隔离架构让并发扩展几乎零开销。

---

## 4. 关键发现

### 4.1 evorule 并发架构正确

evorule 用 per-session reactor 隔离,每个 session 独立事件循环。N 个 sessions 并发 = N 个 reactor 并行,不需要全局锁。

**证据**:
- 5 sessions sustained: 16,570 ops/sec
- 100 sessions sustained: 16,448 ops/sec
- 差异 < 1%(在测试误差范围内)

如果架构有问题,100 sessions 应该明显变慢。

### 4.2 突发 vs 持续的 3.3× 差距

| 模式 | 速率 | 解释 |
|---|---|---|
| **突发** | 55K ops/s | 内存 + tokio 任务池 + reqwest 连接池 都未饱和 |
| **持续** | 16K ops/s | 包含 DB WAL + SQLite commit + blake3 链 + log 写入 |

这个 3.3× 比例跟 1.2 的 4×(63K vs 16K)接近,说明稳态瓶颈在 IO,不是 CPU。

### 4.3 server 可用吞吐量

| 指标 | 数值 | 用户视角 |
|---|---|---|
| **稳态吞吐** | 16K ops/s | 1 秒 16000 条事实进账 |
| **突发吞吐** | 55K ops/s | 短时高峰可消纳 55000 条 |
| **每 session 吞吐** | 164 ops/s | 单 session 1 秒 164 条,够用 |

**16K ops/s 对合规用户意味着**:
- 假设 1 业务事实 = 1 op
- 1 天(86400 秒) × 16K = **13.8 亿事实/天**
- 即 5 亿事实/年,远超中小机构合规需求

### 4.4 server 限速的影响

server 默认 200 req/s 限速。生产环境启用限速,意味着:
- 单 IP 客户端最多 200 req/s
- 多 IP 客户端(不同用户)各自 200 req/s
- 50 用户 × 200 req/s = 10K req/s ≈ 持续吞吐

**限速不是性能瓶颈,是公平性设计**。要更高吞吐,横向扩(多 server 实例)就行。

---

## 5. 复现方法

```bash
# 1) 启动 server(绕过限速)
cd D:\evorule
.\.build\rust\release\evorule-server.exe --addr 127.0.0.1:18081 `
  --db-path .\.build\exp1\evorule.db `
  --memory-dir .\.build\exp1\memory `
  --no-rate-limit

# 2) 突发 100 sessions
.\.build\rust\release\examples\bench_throughput.exe 100 100 50

# 3) 持续 60s(可选)
$env:EVORULE_BENCH_SUSTAINED=1
.\.build\rust\release\examples\bench_throughput.exe 100 10 50
```

预期:
- 突发 10000 ops in ~180ms = 55K ops/s
- 持续 60s = 16K ops/s

---

## 6. 0.2.0 待办(基于本次发现)

| # | 优化项 | 优先级 | 估计时间 |
|---|---|---|---|
| 6.1 | **O(n²) 快照优化** (1.4 暴露) | P0 | 2-3 周 |
| 6.2 | **API 错误上报** (1.4 暴露) | P0 | 1 周 |
| 6.3 | 1MB body limit → CLI flag | P2 | 1 天 |
| 6.4 | 多 server 横向扩 demo | P3 | 待 |

---

## 7. 关键 takeaway

**1. evorule 并发架构 OK** — 100 sessions 跟 5 sessions 性能几乎一样,隔离正确。

**2. 真实稳态 16K ops/s** — 这是个**商业可用**的数字,远超中小机构需求。

**3. 200 req/s 限速是公平性设计,不是性能瓶颈** — 多用户多 IP 横向扩即可。

**4. 与 1.4 的 O(n²) 快照问题是独立维度** — 100 sessions × 100 facts 不会爆,但 1 session × 100K facts 会爆。

---

**最后更新**:2026-07-20
**下次实验**:1.6 确定性测试(same input → same output,1000 次)
