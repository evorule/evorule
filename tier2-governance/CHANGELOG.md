# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-24

首次公开版本。tier2-governance 是 EvoRule 的 HTTP API 和 I/O 治理层。

### 核心能力

- **HTTP API**（axum）— 19 个端点，覆盖会话管理、规则加载、状态查询、调试控制
- **SSE 事件流** — 心跳 + 空闲超时 + 连接数限制
- **5 种 I/O 类型** — call_external / query_db / http_get / save_memory / call_service
- **3 个 I/O handler** — db_handler / http_handler / memory_handler
- **Auditor** — BLAKE3 哈希链 + 逻辑时钟，确保审计不可篡改
- **Cluster** — 多反应器协作管理
- **Hot reload** — 业务规则 watch + 自动加载
- **优雅退出** — SIGTERM/SIGINT 处理，30s 超时
- **独立二进制** — `evorule-server`，支持 JSON 配置、环境变量、CLI 参数

### 配置

- 配置文件：`evorule.json`（从 `evorule.toml` 迁移）
- 环境变量前缀：`EVORULE_`
- 优先级：CLI > 环境变量 > 配置文件 > 内置默认值

---
