# tier2-governance

> EvoRule 三层架构的 Tier 2 治理层 —— I/O 订阅者、审计链、HTTP API。

- **版本**:v0.1.0-alpha.1
- **依赖**:tier0-tcb + tier1-reactor (路径依赖)
- **协议**:AGPL-3.0-or-later

> ⚠️ **本目录不发 crates.io**(`Cargo.toml` 设 `publish = false`)。
> 唯一分发渠道:[Gitee](https://gitee.com/evorulelab/evorule)。

## 概述

`tier2-governance` 是 EvoRule 治理层,负责:

- **I/O 订阅者**:消费 `Fact::IoRequest` → 实际执行(LLM / HTTP / SQL) → 产生 `Fact::IoResponse`
- **审计链**:blake3 哈希链 + gzip 压缩的 append-only JSONL 事实日志
- **HTTP API**:axum 0.8 实现的 REST API(命令 / 状态 / 审计 / 时间旅行)
- **CLI**:独立的 `evorule-server` binary(本目录下)
- **Time-Travel Debugger**:v0.5 调试器 SDK(单文件 HTML)

## 模块结构

```
src/
├── api/            # HTTP API (axum)
├── auditor.rs      # 审计器
├── cluster.rs      # 集群广播
├── config.rs       # 配置加载
├── hash.rs         # blake3 哈希链
├── io_handlers/    # I/O 订阅者实现
│   ├── llm/        #   LLM 调用
│   ├── http/       #   HTTP GET
│   └── db/         #   SQLite 查询
├── state.rs        # 治理层状态
├── time_machine.rs # 时间机器(fork / rewind / replay / diff)
└── bin/
    └── evorule_server.rs  # CLI binary
```

## 编译与运行

```bash
# 仅 governance 服务
cargo run -p tier2-governance --bin evorule-server

# CLI 工具(独立 crate,在 evorule-cli/ 下)
cargo run -p evorule-cli -- <subcommand>
```

## API 端点

主要 HTTP 端点(端口 18081):

- `POST /command` — 发送业务指令
- `GET /state` — 查询当前状态
- `GET /audit` — 拉取审计链
- `POST /audit/verify` — 验证审计链完整性
- `POST /replay` — 重放审计链
- `POST /rewind/{version}` — 回滚到指定版本
- `POST /diff` — 计算状态差异
- `POST /fork/{parent}` — 创建分叉会话
- `GET /debugger/` — Time-Travel Debugger v0.5

## 协议与分发

**代码**:`AGPL-3.0-or-later`(见 [`LICENSE`](LICENSE))。

**分发**:本目录 `Cargo.toml` 设 `publish = false`,**仅通过 Gitee
分发**(https://gitee.com/evorulelab/evorule),**不上 crates.io**。
