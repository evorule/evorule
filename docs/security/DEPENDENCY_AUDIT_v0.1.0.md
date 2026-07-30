<!--
  Copyright 2026 EvoRule Project
  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Dependency Audit — EvoRule 仓 v0.1.0

> **Status**: v0.1.0 baseline（走神 9 拆分后首发，cargo-audit 实跑）
> **Date**: 2026-07-30
> **Scope**: evorule 仓（走神 9 拆分后范围 = 3 lib + evorule-cli）
> **Methodology**: `cargo audit -D warnings`（公网最新 RustSec DB）
> **Previous**: [DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md](DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md)（2026-07-20 生态全栈版，手动审查）

---

## 0. 摘要

> **依赖审计结果：0 CVE，0 warnings，exit=0**
> v0.1.0（走神 9 拆分后首发版）相比 2026-07-20 生态全栈预览版最大变化：① cargo-audit 实跑通过（旧生态版因 rustc 版本不够装不上，回退手动审查）；② H5/H6 迁移后 evorule-governance 依赖大幅瘦身（io_handlers / HTTP / metrics 全迁出至 evorule-application 仓），运行时攻击面显著收窄。

---

## 1. 范围（走神 9 后 evorule 仓独立）

```text
evorule/                 （evorule 仓 = 纯引擎）
├── evorule-tcb/           零外部依赖（no_std 兼容）
├── evorule-reactor/       evorule-tcb + tokio + tracing + serde_json + blake3 + async-trait
├── evorule-governance/    evorule-tcb + evorule-reactor + tokio + tracing + blake3
│                          + flate2 + serde + serde_json + thiserror
└── evorule-cli/           evorule-tcb + serde + serde_json + clap + thiserror + tracing
```

**evorule 仓不包含**（走神 9 / H5 迁出，详见 evorule-application 仓）：

- `sqlx` / `reqwest` → evorule-application 仓的 io_handlers
- `axum` / `tower` / `tower-http` / `tower_governor` / `governor` / `subtle` → evorule-application 仓的 evorule-server
- `prometheus` → evorule-application 仓的 evorule-server metrics 实现
- `async-stream` / `futures-core` → evorule-server（SSE 流式端点，应用层）
- `notify` → 已移除（hot_reload 删除）

## 2. cargo-audit 实跑结果（B-2 验收）

| 项         | 值                                                    |
| ---------- | ----------------------------------------------------- |
| 命令       | `cargo audit -D warnings`                             |
| RustSec DB | 公网最新（1,173 advisories）                          |
| 扫描范围   | 231 crates（含 dev-deps / bench-deps / all-features） |
| CVE 命中   | **0**                                                 |
| warnings   | **0**                                                 |
| exit code  | **0**                                                 |

> v0.1.0 时 `cargo-audit` 安装被 `kstring@2.0.4 requires rustc 1.96.0` 阻塞（当时 rustc 1.92.0），回退手动审查。v0.2.0 工具链升级后实跑通过。

## 3. 核心运行时依赖清单（从 Cargo.toml）

| Crate              | 依赖                                                               | 说明                           |
| ------------------ | ------------------------------------------------------------------ | ------------------------------ |
| evorule-tcb        | （零外部依赖）                                                     | no_std 兼容内核，Kani 验证通过 |
| evorule-reactor    | tokio / tracing / serde_json / blake3 / async-trait                | 反应式执行引擎                 |
| evorule-governance | tokio / tracing / blake3 / flate2 / serde / serde_json / thiserror | 治理层（瘦身后）               |
| evorule-cli        | serde / serde_json / clap / thiserror / tracing                    | CLI 工具（publish=false）      |

> 具体锁定版本见 `Cargo.lock`。所有依赖均为当前稳定版，无 known CVE 命中。

## 4. v0.1.0 → v0.2.0 依赖瘦身对比

| 维度                        | v0.1.0                                                 | v0.2.0                            | 变化                    |
| --------------------------- | ------------------------------------------------------ | --------------------------------- | ----------------------- |
| evorule-governance 直接依赖 | ~25（含 sqlx/reqwest/axum/tower/prometheus/notify...） | 9                                 | ⬇️ 攻击面大幅收窄       |
| HTTP / DB / 网络依赖        | 在 governance（io_handlers）                           | 迁出 evorule 仓                   | ✅ 核心不再含网络攻击面 |
| metrics 依赖                | prometheus 在 governance                               | 迁出（feature flag / 应用层注入） | ✅ 嵌入式场景依赖更轻   |
| 热重载依赖                  | notify                                                 | 移除                              | ✅                      |

## 5. 已知历史 CVE（均已 patched，v0.1.0 沿袭）

| Advisory          | Crate  | Our status                                      |
| ----------------- | ------ | ----------------------------------------------- |
| RUSTSEC-2024-0362 | sqlx   | n/a（已迁出 evorule 仓，在 application 仓审计） |
| RUSTSEC-2025-0020 | flate2 | ✅ 1.1.9（governance 仍用，gzip 审计链压缩）    |
| RUSTSEC-2024-0381 | tokio  | ✅ 1.52+                                        |

## 6. 建议（0.x 阶段）

- 把 `cargo audit -D warnings` 纳入 CI 强门禁（`.gitee-ci/validate.yml` + `.github/workflows/ci.yml`）—— 已接线，待首次 push 真实验证
- 考虑加 `cargo-deny`（许可证白名单 + 重复依赖检测 + 来源白名单）
- 3 个月未更新依赖评估（v0.2.0 已做，0 高风险）

## 7. 结论

> **v0.2.0 依赖审计：0 CVE，0 warnings**
> cargo-audit 公网最新 DB（1,173 advisories）扫描 231 crates 无命中。
> H5/H6 迁移后核心运行时依赖大幅瘦身，网络 / DB / HTTP 攻击面迁出 evorule 仓。
> **v0.2.0 从依赖安全角度可发布。**

---

## 8. Change Log

| Version | Date       | Change                                                                       |
| ------- | ---------- | ---------------------------------------------------------------------------- |
| 0.2.0   | 2026-07-30 | 首次 cargo-audit 实跑（0 CVE）；H5/H6 依赖瘦身；走神 9 后聚焦 evorule 仓范围 |
| 0.1.0   | 2026-07-20 | 手动审查（cargo-audit 装不上）；见 [v0.1.0](DEPENDENCY_AUDIT_v0.1.0.md)      |
