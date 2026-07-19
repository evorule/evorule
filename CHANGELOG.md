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

## [0.1.0] - 2026-07-20

项目重启 / 公开起点。`evorule` 生态从 0.1.0 重新开始版本号(原内部版本 6.0.x 退役,无 GitHub/Gitee 撤回成本)。

### 🆕 新增

- **VERSION_STRATEGY.md** (v1.1) — 生态版本号标准,12 章节
- **5 个 validate-*.ps1 + validate-all.ps1** — SemVer / CHANGELOG / License / Cargo.lock / Tag 校验
- **.gitee-ci/validate.yml** — Gitee Go 流水线
- **.gitee/PULL_REQUEST_TEMPLATE.md** — 人类审查 checklist(分 4 档:通用 / SDK / 升 1.0 / 第三方审计)
- **D:\evorule\文档\pre-0.1.0-checklist.md** — 发版前单一真相源

### 🔄 变更

- **协议统一为 AGPL-3.0-or-later**(原 `MIT OR Apache-2.0`)
- **core_eval.json 标注 CC0-1.0** — 宪法规范与代码协议分离
- **所有 .rs 文件加 SPDX header** — 65 个文件全覆盖
- **统一基线版本 0.1.0** — 7 个项目(workspace + tier0/1/2 + evo-agent + sdk-ts + sdk-py)同步

### 🐛 修复

- **sdk-python license 错标为 MIT** → 修正为 AGPL-3.0
- **evorule CHANGELOG 章节格式** — `## [v6.0.0]` → `## [6.0.0]`(脚本可校验)
- **CHANGELOG 各项目版本章节** — evorule/evo-agent 新增 0.1.0 段

### ⚠️ Breaking Changes

- **版本号从 6.0.x 重启为 0.1.0** — SemVer 0.x 阶段,API 不承诺兼容
- **文档/ 永不发布** — 内部文档隔离,仅对外通过 docs/ 整理

---

## [未发布] - v6.1.0

### 🆕 新增

- **TypeScript SDK** (`@evorule/sdk` v6.0.0) - 19 个 HTTP 端点完整封装
  - `EvoruleClient` + `Session` + `Event` 类
  - SSE 事件流订阅
  - 5 种异常类型(EvoruleError / AuthenticationError / SessionNotFoundError / SessionClosedError / CommandError)
- **端到端冒烟测试** (`tests/e2e_smoke.py`) - 5 场景覆盖宪法核心
- **发布套件** - 9 个 .md 文件,中文为主,英文版本(LICENSE / README / CONTRIBUTING_ZH / CONTRIBUTING / CODE_OF_CONDUCT / SECURITY / CHANGELOG / NOTICE / AUTHORS)
- **CLA-individual.md** - 个人贡献者许可协议 v1.0(基于 evorule-core-backup 风格)
- **`core_eval.json` CC0-1.0 标识** - 宪法元数据标注为公共领域
- **品牌协议策略** - 代码 AGPL-3.0-or-later,`core_eval.json` CC0-1.0
- **完整 SPDX header** - 47 个 .rs 文件加 SPDX 标识

### 🔄 变更

- **协议统一为 AGPL-3.0**(原 evorule 整体为 `MIT OR Apache-2.0`,SDK 为 `MIT`)
- **README 重写** - 突出"只接受和运行 JSON 数据集"哲学
- **evorule.toml 配置 → evorule.json**(`tier2-governance` 启动配置)

### 🐛 修复

- **sqlx 0.8 API 适配** - `from_str` → `parse`,`SqlitePool::builder()` → `SqlitePoolOptions::new()`
- **evorule_server.rs 重复 spawn 块** - 删除 14 行复制粘贴

### ⚠️ Breaking Changes

- **TOML 配置 → JSON 配置**:`evorule.toml` 用户需手动迁移到 `evorule.json`
- **协议变更**:`MIT OR Apache-2.0` → `AGPL-3.0`(内部已有用户需知会)

---

## [6.0.0] - 2026-07-19

### 🆕 新增

#### 三层架构

- **tier0-tcb** (~5200 行)
  - 纯计算内核,`#![no_std]` 兼容,零外部依赖
  - 7 个域类型:Boolean / Integer / Decimal / String / Array / Object / Null
  - 4 个元指令:set / push / branch / io_request
  - 6 个 Kani proof stub(verify_value_roundtrip / verify_path_no_panic 等)
  - build.rs 编译时门禁(G8 强制)

- **tier1-reactor** (~9900 行)
  - 反应器主循环(drain → stable → block → execute)
  - Fact 枚举 7 个变体(固定不变)
  - FactsLog(append-only)+ WAL
  - 因果链(`cause: FactId`)
  - 时间机器(replay / rewind / fork / diff)
  - 调试控制(pause / resume / step)

- **tier2-governance** (~8300 行)
  - HTTP API(axum) — 19 端点
  - SSE 事件流(心跳 + 空闲超时 + 连接数限制)
  - 5 种 I/O 类型(call_external / query_db / http_get / save_memory / call_service)
  - 3 个 I/O handler(db_handler / http_handler / memory_handler)
  - Auditor(BLAKE3 哈希链 + 逻辑时钟)
  - Cluster(多反应器协作)
  - Hot reload(业务规则 watch + 自动加载)
  - 优雅退出(SIGTERM/SIGINT,30s 超时)
  - 独立二进制 `evorule-server`

#### 关键能力

- **可审计执行链** - 所有变化进 FactsLog,可回放
- **时间机器** - rewind 到任意历史点,fork 实验
- **JSON 唯一表达** - 规则 / 状态 / 事件 / I/O 全部 JSON
- **可热重载业务规则** - 监听 watch 目录,自动加载

---

## [历史版本] - evorule-core-backup v0.2.0-beta(2026-05-18)

早期 Python 版本,作为 EvoRule Rust 版的设计来源。

**API 升级**:
- `Domain.__hash__()` - 新增哈希方法
- License 从 Apache-2.0 更新为 AGPL-3.0-or-later

**文档**:
- `CLA-individual.md` - 个人贡献者协议
- `COMMERCIAL_EXEMPTION.md` - 商业豁免协议说明
- `FREE_COMMERCIAL_LICENSE.md` - 免费商业许可说明
- `SECURITY.md` - 安全政策
- `CONTRIBUTING_ZH.md` - 中文贡献指南
- `docs/licensing/` - 许可证 FAQ

**修复**:
- 统一版本号为 0.2.0b0
- 移除 `evorule_quality_cache` 从 Git 追踪

⚠️ License 变更为 AGPL-3.0-or-later

---

## 未来规划

### v6.1(近期)
- 真实 LLM 集成(OpenAI 兼容协议)
- Tier 1 完整 Kani 验证
- 移除过时的 `replay` API 数组格式(统一为 `{"facts": []}`)

### v6.2(中期)
- Cluster 多反应器 → Raft 共识
- 业务规则 DSL v2(更可读)

### v7.0(远期)
- 反应式规则(LLM 生成 + 规则自动反应)

---

**作者**: EvoRule Project
**邮箱**: evorulelab@gmail.com
**Gitee**: https://gitee.com/evorulelab/evorule

---

**本变更日志遵循 [evorule-core-backup](https://gitee.com/evorulelab/evorule-core) 的发布风格,采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
