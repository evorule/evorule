<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [v0.1.0] - 2026-07-25 (EvoRule 公开 baseline)

随 [`evorule` v0.1.0](https://gitee.com/evo-rule-lab/evorule) 同步发布,作为
evorule workspace 内的子 crate。

### 能力

- ✅ **零网络** —— 不调用任何外部服务
- ✅ **零遥测** —— tracing 只写 stderr
- ✅ **零 AI 决策** —— 不调用 LLM,纯确定性执行
- ✅ **零系统依赖** —— musl 静态链接,1.6 MB 单文件
- ✅ **多架构** —— `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl`
- ✅ **可重现构建** —— `build-musl.sh --repro` 两次构建 SHA256 一致
- ✅ **G8 门控** —— 编译期拦截"硬编码控制流"违规(与 tier1/tier2 同套规则)
- ✅ **e2e 测试** —— `tests/e2e.sh` 15/15 PASS

### 4 个子命令

| 子命令 | 用途 |
|---|---|
| `evorule validate <rules-dir>` | 校验 JSON 规则 schema |
| `evorule run <rules-dir>` | 执行规则 + 输出 fact log(JSONL) |
| `evorule replay <fact-log>` | 重放 fact log(pretty-print) |
| `evorule diff <a.log> <b.log>` | 对比两个 fact log |

### 已知限制(0.1.0)

- ❌ 无 I/O handler(MVP 只做 `noop` + state transition)
- ❌ 无 HTTP API(那是 `evorule-server` 的事,在 `tier2-governance`)
- ❌ 无配置文件(后续加 `.evorule.toml`)
- ❌ 无 hot-reload(后续加)

### 配套示例

- [`examples/hospital/`](examples/hospital/) —— 医院 HIPAA / 等保 2.0 合规规则
- [`examples/law-firm/`](examples/law-firm/) —— 律所客户保密 / GDPR 合规规则

详见 [`README.md`](README.md) + [`CLI_SPEC.md`](CLI_SPEC.md)。
