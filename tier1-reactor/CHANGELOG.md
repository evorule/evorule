# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-24

首次公开版本。tier1-reactor 是 EvoRule 的反应器引擎。

### 核心能力

- **核心反应器引擎** — drain → stable → block → execute 四阶段循环
- **Fact 通道通信** — 7 个 Fact 变体（Command / PayloadUpdate / StateTransition / IoRequest / IoResponse / Stable / Error）
- **FactsLog** — append-only 审计链 + WAL，因果链（`cause: FactId`）
- **时间机器** — fork / rewind / replay / diff
- **调试控制** — pause / resume / step / check_and_wait
- **ReactorCluster** — 多反应器协作（join / channel / shared_facts_space）
- **C FFI 接口**（`ffi` feature）— 8 个导出函数：
  - `evorule_reactor_new()` / `evorule_reactor_free()`
  - `evorule_reactor_send_command()`
  - `evorule_reactor_pause()` / `evorule_reactor_resume()` / `evorule_reactor_step()`
  - `evorule_reactor_current_queue_size()` / `evorule_reactor_is_paused()`
  - `evorule_version()`

### 形式化验证

- **7 个纯逻辑函数**（pure.rs）：`next_step` / `apply_command` / `apply_payload_update` / `apply_io_response` / `check_invariants` / `is_stable` / `register_io_request_pure`
- **5 个 Kani 验证目标桩**

### 安全契约

- 编译时门禁检查（G8 强制）
- I/O 超时阈值策略
- 运行时指标
- `ReactorStateSnapshot` 包含 `queue_snapshot` / `pending_io_snapshot` / `is_paused` / `step_quota` 字段
- `ReactorHandle` 提供 7 个 API 扩展

---
