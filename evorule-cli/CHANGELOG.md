# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [v0.1.0-alpha.1] - 2026-07-20 (EvoRule 公开 alpha)

EvoRule 项目化重塑首版
- 品牌: TheEquation → EvoRule
- 协议: MIT OR Apache-2.0 → AGPL-3.0-or-later (项目统一)
- `Cargo.toml` `publish = false`,musl 静态链接
- 详见根 [README.md](../../README.md) + [CLI_SPEC.md](CLI_SPEC.md)

## [v0.1.0] - 2026-07-20 (EvoRule 公开 baseline)

跟 [v0.1.0-alpha.1] 同源,正式公开。
- 本地 JSON 规则执行器,单 binary < 2MB(musl 静态)
- 零网络、零 telemetry,面向"圈 2 合规刚需"用户
- `Cargo.toml` `publish = false`
