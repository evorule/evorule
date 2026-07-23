# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 阶段9：C FFI 接口（`ffi` feature）
  - `evorule_reactor_new()` - 创建反应器
  - `evorule_reactor_free()` - 销毁反应器
  - `evorule_reactor_send_command()` - 发送命令
  - `evorule_reactor_pause()` / `evorule_reactor_resume()` - 暂停/恢复
  - `evorule_reactor_step()` - 执行指定步数
  - `evorule_reactor_current_queue_size()` - 查询队列大小
  - `evorule_reactor_is_paused()` - 查询暂停状态
  - `evorule_version()` - 获取版本号

### Changed

- 添加 `cdylib` crate-type 支持动态链接库编译

## [v0.1.0-alpha.1] - 2026-07-20 (EvoRule 公开 alpha)

EvoRule 项目化重塑首版
- 品牌: TheEquation → EvoRule
- 协议: MIT OR Apache-2.0 → AGPL-3.0-or-later (项目统一)
- 仅 Gitee 分发(`publish = false`)
- 详见根 [README.md](../../README.md) + [REACTOR_SPEC.md](REACTOR_SPEC.md)

## [v0.1.0] - 2026-07-20 (EvoRule 公开 baseline)

跟 [v0.1.0-alpha.1] 同源,正式公开。
- 全部功能继承自历史版本
- 仅 Gitee 分发(`publish = false`)

## [6.0.0] - 2026-07-17

### Added

- 阶段8：协作原语
  - `ReactorCluster` - 多 reactor 协作管理
  - `join(a, b)` - 建立双向状态同步
  - `channel(a, b)` - 消息通道
  - `shared_facts_space()` - 共享内存区域
  - `SyncIdRingBuffer` - 循环检测环形缓冲区

- 阶段7：Kani 形式化验证准备
  - `pure.rs` - 纯逻辑模块
  - 7 个纯逻辑函数：`next_step`, `apply_command`, `apply_payload_update`, `apply_io_response`, `check_invariants`, `is_stable`, `register_io_request_pure`
  - 5 个待验证目标桩

- 阶段6：调试器级可观测性
  - `DebugControl` - 调试控制结构体
  - `pause()` / `resume()` / `step(n)` - 控制原语
  - `check_and_wait()` - 防死锁检查
  - `current_queue()` / `pending_io()` - inspect API

### Changed

- `ReactorStateSnapshot` 新增 `queue_snapshot`, `pending_io_snapshot`, `is_paused`, `step_quota` 字段
- `ReactorHandle` 新增 7 个 API

## [5.0.0] - 2026-07-17

### Added

- 阶段5：时间机器
  - `fork()` - 分叉新会话
  - `rewind()` - 回滚到历史状态
  - `replay()` - 重放审计链
  - `diff()` - 计算状态差异

## [4.0.0] - 2026-07-17

### Added

- 阶段4：审计链与记忆系统融合
  - `FactsLog` - Append-Only 审计链
  - `PayloadUpdate` - 负载更新 Fact
  - 消息滑动窗口
  - 长期记忆管理

## [3.0.0] - 2026-07-17

### Added

- 阶段3：可观测性增强
  - I/O 超时阈值策略
  - 运行时指标
  - 编译时门禁检查

## [2.0.0] - 2026-07-17

### Added

- 阶段2：执行引擎增强
  - 稳定检测
  - 状态转换逻辑
  - 错误处理

## [1.0.0] - 2026-07-17

### Added

- 初始版本
  - 核心反应器引擎
  - Fact 通道通信
  - 基本状态管理