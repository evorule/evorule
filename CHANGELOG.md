<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 更新日志

所有对 EvoRule 项目的重大更改都将记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0,
本项目遵循 [语义化版本控制](https://semver.org/lang/zh-CN/) v2.0。

徽章说明:

- 🆕 新增
- 🔄 变更
- 🐛 修复
- 🗑 弃用
- ⚠️ Breaking Change
- 🔒 安全

---

## [0.1.0] - 2026-07-24

项目首次公开版本。evorule 是一个反应式规则执行框架，采用三层架构（TCB / Reactor / Governance），提供确定性执行、可审计链、时间旅行调试。

### 🆕 新增

#### 三层架构

- **tier0-tcb** — 纯计算内核，`#![no_std]`，零外部依赖
  - 7 个域类型（Boolean / Integer / Decimal / String / Array / Object / Null）
  - 4 个元指令（set / push / branch / io_request）
  - build.rs 编译时门禁（14 条 redline，G8 强制）
  - 5 个 Kani proof（4 PASS + 1 TIMEOUT），19 个 proptest
  - `MAX_TRANSFORM_RULES = 64` 限制

- **tier1-reactor** — 反应器主循环
  - drain → stable → block → execute 四阶段
  - FactsLog（append-only）+ WAL，因果链
  - 时间机器（replay / rewind / fork / diff）
  - 调试控制（pause / resume / step）
  - ReactorCluster 多反应器协作（join / channel / shared_facts_space）
  - C FFI 接口（`ffi` feature）：8 个导出函数

- **tier2-governance** — HTTP API + I/O 治理层
  - 19 个 HTTP 端点，SSE 事件流（心跳 + 空闲超时 + 连接数限制）
  - 5 种 I/O 类型，3 个 I/O handler（db / http / memory）
  - Auditor（BLAKE3 哈希链 + 逻辑时钟）
  - Hot reload（业务规则 watch + 自动加载）
  - 优雅退出（SIGTERM/SIGINT，30s 超时）
  - 独立二进制 `evorule-server`

#### 生态基础

- **VERSION_STRATEGY.md** — 生态版本号标准（12 章节）
- **5 个 validate-\*.ps1 + validate-all.ps1** — SemVer / CHANGELOG / License / Cargo.lock / Tag 校验
- **.gitee-ci/validate.yml** — Gitee Go 流水线
- **TypeScript SDK**（`@evorule/sdk` 0.1.0）— 19 个 HTTP 端点封装，SSE 事件流，5 种异常类型
- **Python SDK**（`evorule` 0.1.0）— 完整客户端封装
- **端到端冒烟测试** — 5 场景覆盖宪法核心

### 🔒 安全

- **M1 Bearer token** — `--auth-token` / `EVORULE_AUTH_TOKEN`，非 loopback 启动警告
- **SECURITY_AUDIT v0.1.0** — 4 medium issues 全 closed，11 LOW documented
- **THREAT_MODEL.md** — 14 章节，7 attack trees，STRIDE per component
- **DEPENDENCY_AUDIT v0.1.0** — 25 deps，0 known CVEs

### 📚 文档

- **5 原则 DESIGN_PRINCIPLES** — 透明 / 可选 / 可控 / 可回放 / 可审计
- **`core_eval.json` 标注 CC0-1.0** — 宪法规范与代码协议分离
- **ROADMAP.md** — 公开路线图
- **STATUS.md** — 公开当前状态

### 🔄 变更

- **协议统一为 AGPL-3.0-or-later**（原 `MIT OR Apache-2.0`）
- **所有 .rs 文件加 SPDX header** — 95 个文件全覆盖

### 🐛 修复

- **sdk-python license 错标为 MIT** → 修正为 AGPL-3.0
- **Clippy 警告修复** — 移除未使用的导入、无用的 enumerate、不必要的 borrow、clone 优化、len 比较优化
- **代码格式化修复** — 统一 `#[command]` 属性格式、移除无用的 `vec!`

### 已知问题

- ❌ **Gitee Go CI 未真跑过** — `.yml` 写了但没 push 验证
- ❌ **跨平台 release 未真测过** — 只跑过 Windows + localhost
- 🟡 **Kani `verify_path_no_panic` TIMEOUT** — Kani 工具链 alloc std unwind bound 限制，proptest 保底
- ❌ **依赖自动审计缺** — `cargo-audit` 装不上（rustc 1.92 < 1.96）

详见 [STATUS.md](STATUS.md) §"已知问题"。

---

**作者**: EvoRule Project
**邮箱**: evorulelab@gmail.com
**Gitee**: https://gitee.com/evorulelab/evorule

---

**本变更日志采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
