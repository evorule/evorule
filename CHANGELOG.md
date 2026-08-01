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

## [0.1.1] - 2026-08-01

**evorule-tcb / evorule-governance patch 发布** — `exec_set` 路径解析诊断增强 + 中间节点/空列表语义收紧；附带 evorule-reactor clippy `indexing_slicing` 修复。四 crate 工作区版本同步 bump（cli 无代码改动，仅版本同步）。

### 🐛 修复

- **`exec_set` 路径校验加强** — `parts.first() == Some(&"")` → `parts.iter().any(|p| p.is_empty())`，捕获 `"a..b"` / `"a.b."` 等中间/尾部空段（原仅查首段）
- **`exec_set` 中间节点语义收紧** — null/缺失自动建空对象继续 descend；其他非对象类型（integer/boolean/string/array）返回 `PathResolutionFailed` 且**不覆盖原值**（原对 null 报错、对其他非对象静默覆盖，语义不一致）
- **`exec_push` 空列表改 no-op** — `EmptyInstructionList` 错误 → `Ok(state)`，支持合法的空 `else: []` / `then: []`（变体保留以维持错误类型 API 稳定）
- **`resolve_instructions_list` 数组展平** — 路径引用解析为数组时 `extend` 展平入队，修复 `body`/`then`/`else` 为指令列表时的入队错误（原整包 push 导致执行失败）
- **evorule-reactor clippy `indexing_slicing` 修复** — `facts_log.rs` 两处 `history[start..]` → `history.get(start..).unwrap_or(&[])`（`start` 由 `unwrap_or(len)` / `saturating_sub` 保证 ≤ len）；`reactor.rs` 一处 `&parts[0..len-1]` → `parts.get(..len-1).unwrap_or(&[])`（`else` 分支保证 `len ≥ 2`）；`facts_log.rs` 测试模块补 `#![allow(clippy::indexing_slicing)]`（与既有 `unwrap_used`/`panic` 豁免一致）。修复 Windows 工具链下 clippy 阻塞（v0.1.0 的"clippy 全绿"为 WSL 环境结果），逻辑不变

> **行为变更说明**：上述修复中，null 中间节点与空列表 push 从"报错"变为"成功"。这是修正原错误行为（合法输入被拒），非破坏性变更；错误**类型**未变（`EmptyInstructionList` 变体保留），公开 API 签名未变。

### 🆕 新增

- **`PathResolutionFailed` 诊断消息丰富** — 含四要素：失败路径 + 出问题的段名 + 实际类型 + 期望类型，供上层日志直接 `Display` 输出（TCB 本身仍零日志副作用）
- **`IoDispatcher` 实现 `Clone`** — 内部全为 `Arc<dyn IoHandler>`，clone 仅增引用计数
- **`SessionManager` 新增 `core_eval()` getter + `replace_core_eval()` 原子替换** — 仅影响新会话，已运行会话的 TCB 不可变语义不变
- **集成测试 `evorule-tcb/tests/set_path_diagnostics.rs`** — 8 场景锁定诊断契约（integer/boolean/array/string 中间节点报错、null 自动创建、payload 根异常、空段）

### 📚 文档

- **TCB_SPEC.md 新增 §八「错误语义与路径解析诊断契约」** — 含中间节点自动创建策略表、诊断消息四要素、可执行规约引用；原 §八代码量顺延为 §九
- **`transition.rs` 注释更新** — `EmptyInstructionList` 标注为保留变体

### ✅ 验证

- `cargo test --workspace` 全通过（默认 features + `--features evorule-reactor/persistence` 双跑）
- `cargo clippy --workspace --all-targets -- -D warnings` 0 warnings（默认 features + persistence feature 双跑）
- `cargo build --release` 通过
- `scripts/validate-all.ps1 -AllowUnreleased` 5 项全通过（version / changelog / license / cargolock / release）
- `evorule-tcb`：168 lib 单元 + 集成（含新增 `set_path_diagnostics` 8 场景）+ 17 doc-tests 全通过
- `evorule-governance`：全通过
- `scripts/check_doc_safety.py` 全过（无私有路径泄露、L1 交叉引用完整）

> 注：`ffi` feature 在 `--all-features` 下因 `lib.rs` 的 `#![forbid(unsafe_code)]` 与 `ffi.rs` 的 `#![allow(unsafe_code)]` 冲突（E0453）无法编译，此为 v0.1.0 预存在问题，与本版本修复无关，不在 0.1.1 范围内。

---

## [0.1.0] - 2026-07-30

**evorule 仓首次公开发布** — 走神 9 决策:evorule 仓必须独立 release，不绑 application/agent 完成。
3 个核心 lib crate 改名后真发到 crates.io：`evorule-tcb` / `evorule-reactor` / `evorule-governance`。

### 🆕 新增

- **品牌统一改名** — `tier0-tcb` → `evorule-tcb`，`tier1-reactor` → `evorule-reactor`，`tier2-governance` → `evorule-governance`
  - 改 3 个 Cargo.toml `name` 字段
  - 改 workspace `members` 段
  - 改 87 个 Rust 源文件，304 处 `use` 语句
  - 改 ~1300 处连字符形式引用（CI / 脚本 / 文档 / README / AGENTS）
  - 子 crate 依赖改为 `path + version = "0.1.0"` 双声明（本地开发 + crates.io 发布两不耽误）
  - MSRV 兼容性：`is_multiple_of`（Rust 1.87+）替换为 `% N == 0`，兼容 MSRV 1.74

- **crates.io 发布元数据补全**
  - 3 个 crate `Cargo.toml` 补全：`name` / `version` / `description` / `license` / `authors` / `homepage` / `documentation` / `readme = "README.md"`（B7 修复路径错）/ `rust-version = "1.74"` / `keywords` (5 个) / `categories` (3-4 个) / `exclude` (N6 清理)
  - 顶层 `Cargo.toml` `[workspace.package]` 段：`version = "0.1.0"` / `authors = ["EvoRule Project"]` / `repository` / `homepage` / `rust-version = "1.74"`
  - `evorule-reactor` keywords 从 6 个删到 5 个（删 `actor-model`，超 crates.io 上限）
  - 顶层 `Cargo.toml` `[profile.release]` 段 GBK 乱码修复为正确 UTF-8 中文

- **AGPL + CC0 协议合规** — 3 个 crate 各补 `NOTICE.md`（AGPL-3.0 §7(b) 归属声明 + `core_eval.json` 的 CC0 协议分离说明），避免下游误判整个包为 AGPL

### 🔄 变更

- **evorule-application 仓** 切到新 `evorule-*` 命名（`path + version = "0.1.0"` 双声明）
- **evo-agent 仓** 切到新 `evorule-*` 命名（`path + version = "0.1.0"` 双声明）
- **`reactive_researcher`** repository URL 统一为带连字符版（`gitee.com/evo-rule-lab/evorule`）
- **kani_proofs 编译目标** — 改名时 Kani metadata 列表同步更新

### 🔒 安全

- 升级到 Kani 0.67.0 + nightly-2025-11-21 后，tier0 3 个 domain Kani proof（eq/lt/exists）首次跑通
  - `verify_evaluate_domain_eq_kani` 67.17s PASS（947 checks, 0 failed, 42 unreachable）
  - `verify_evaluate_domain_lt_kani` 79.62s PASS
  - `verify_evaluate_domain_exists_kani` 57.79s PASS
- tier0 编译时门禁：`#![deny(unwrap_used)]` / `#![deny(expect_used)]` / `#![deny(indexing_slicing)]` / `#![deny(panic)]`
- tier0/1/2 编译时 build.rs 门禁全开（T1/T2/T3/T15）

### 📚 文档

- 「v0.1.0 不发」决策撤销（决策作废，详见撤销说明）
- v0.2.0 标准拆分为 2.1/2.2/2.3 三份（evorule 仓 / application 仓 / agent 仓独立 release-blocker）
- 发布程序备查文档新增 v1.1（crate 改名 + 元数据 + 阻塞型坑 B1-B8 + 非阻塞 N1-N6）
- evorule 发布准备方案、确定性测试方案 — 上游决策记录

### ✅ 验证

- `cargo build --workspace` exit 0
- `cargo test --workspace` exit 0（643 passed, 0 failed, 4 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings` exit 0
- `cargo publish -p evorule-tcb --dry-run --allow-dirty` exit 0（Packaged 28 files, 539.3KiB / 112.5KiB compressed）

### 已知问题

- ⚠️ **v0.1.0 跨平台 release 实测** 仅 Windows + WSL，macOS 待 CI 跑过确认
- ⚠️ **Kani tier0 12 proof 中 9 个未实跑**（仅 3 个 domain proof 跑通），verify_path_no_panic 等 9 个待 Kani 0.68+ 升级后跑通
- ⚠️ **Kani tier1 11 proof 中 4 个新 C1-1~C1-4 未实跑**（代码已就位，等 Kani 0.68+ 跑通）
- ⚠️ **跨平台 release 验证 (F-1)** 状态 3 个 ❌ 待改 ✅
- ⚠️ **Gitee Go CI 真实跑通 (F-2)** 需 push 后才能看到 run ID
- ⚠️ **5 终极门禁** 需打 v0.1.0 tag 前全过

详见 [STATUS.md](STATUS.md) §"已知问题"。

---

### 形式化验证（A-1 / A-2 / C-1）

- **evorule-tcb 12 个 Kani proof**（A-1）— WSL Kani 0.67.0 + CBMC 实跑
  - `verify_value_roundtrip` / `verify_path_no_panic` / `verify_set_integer_safety` / `verify_jsonvalue_array_safety` / `verify_set_sub_safety` / `verify_resolve_path_object_kani` / `verify_evaluate_domain_eq_kani` / `verify_evaluate_domain_lt_kani` / `verify_evaluate_domain_exists_kani` / `verify_execute_transition_kani` / `verify_termination_kani` / `verify_depth_enforcement_kani`
- **evorule-reactor 11 个 Kani proof**（A-2，7 原有 + 4 新 C1-1~C1-4）
  - 7 原有：`invariant_io_count_register_complete` / `invariant_io_count_force_remove` / `invariant_version_monotonic` / `invariant_io_recovery_iff_result` / `command_does_not_decrease_queue` / `max_rounds_termination` / `invariant_cause_queue_sync`
  - 4 新：`proof_fact_log_append_monotonic` / `proof_hash_chain_back_link` / `proof_reactor_invariants_preserved_after_pure_ops` / `proof_phase_state_machine_cannot_jump`
- **C-1 `verify_path_no_panic` Kani 真实通过** — 原 BTreeMap 路径爆炸问题通过基于 Vec 的 KIdSet/KIdMap 替代方案解决

### 引擎质量（A-3 / B-1 / B-2 / E-1）

- **A-3 fact log 索引 + 长期压缩** — 3 个 BTreeMap 索引（version_index / fact_id_index / path_index）加速查询；手动 `compact()` 快照压缩，5000 facts 压缩率 99.98%（>> 40% 阈值）
- **B-1 clippy 0 warnings** — `cargo clippy --workspace --all-targets -- -D warnings` exit=0
- **B-2 cargo audit 0 CVE** — 公网 RustSec DB 实跑 0 CVE
- **E-1 criterion 性能基准** — evorule-tcb 3 组基准（execute_transition / jsonvalue_ops / path_resolve）实跑

### 走神 9 拆分（机制-应用边界清理）

- **evorule 仓独立 release** — 不绑 application/agent 仓；HTTP API / io_handlers / 认证中间件归 evorule-server 独立仓；可视化 / 业务模板归 evorule-application 仓
- **H5 迁移落地** — `evorule-server` 与 `io_handlers` 从 evorule-governance 迁出；evorule-governance 现为纯机制层库（IoDispatcher 框架 + IoHandler trait re-export）
- **走神 9 二次拆分落地** — `evorule-server` 与 `io_handlers` 从 evorule-application 仓再次迁出，成立 **evorule-server 独立仓**（9 个子 crate：server + io_handlers + auth + metrics + hot_reload + time_machine + debug_control + rule_tools + semantic_invariants）；evorule-application 仓回归纯可视化/业务模板定位
- **README 定位调整** — 叙事调整为"通用反应式执行引擎"
- **SDK 全部外移** — TypeScript/Python/Go/Java SDK 迁移到独立仓 evorule-sdk
- **evorule-cli 业务规则模板外移**（2026-07-30）— `evorule-cli/examples/hospital/` + `evorule-cli/examples/law-firm/` 两套行业规则集（HIPAA / 律所合规）共 10 个文件，按 `AGENTS.md` 边界判断表「evorule-cli 加规则模板 = 业务内容，不是机制，放 evorule-application ❌」迁移至兄弟仓 `evorule-application/examples/evorule-cli/`；evorule 核心仓 `evorule-cli/examples/` 仅保留机制层最小化演示 README 与 tests/fixtures 用例
  - 【留痕声明】本次迁移是**经用户明确要求并确认**的越界清理操作，遵循 AGENTS.md 规则二的警告确认流程，不再回迁

### 🗑 移除（H5 边界清理）

- **移除 portal** — 聚合端点属应用层 UI
- **移除 hot_reload** — 业务规则热重载属策略层，移除 `notify` 依赖
- **移除 cluster** — 多反应器协作原语属高级功能，后续在 application 仓实现
- **移除 object_pool** — FactsLog 对象复用优化属性能优化，待后续评估重加
- **metrics / auth 转为 feature flag** — 默认不编译，减少嵌入式场景依赖

### 🐛 修复

- **MSRV** — `is_multiple_of` → `%` 取模（reactor.rs / auditor.rs），兼容 Rust 1.74
- **时间机器 diff 递归** — 从顶层字段对比扩展为嵌套对象递归对比
- **Kani 路径爆炸** — BTreeSet/BTreeMap → KIdSet/KIdMap（Vec 基实现），11 个 reactor proof 全部稳定通过

### 已知问题（v0.1.0 首发时仍存在）

- ⚠️ **P1 安全修复待公网部署前完成** — H6 SSRF / H7 SQL / H8 CORS / H9 DB URL（io_handlers 已迁 evorule-application 仓，在该仓修复）
- ⚠️ **跨平台 release 实测** 仅 Windows + WSL，macOS 待 CI 跑过确认
- ❌ **API 稳定承诺仍不提供** — 1.0 之前 API 仍可能变化

详见 [STATUS.md](STATUS.md) §"已知问题"。

---

## [未发布] — v0.2.0 质量硬化（计划 2026-08-XX）

> 本节为 v0.1.0 首发之后的规划项。

**硬化目标**：在 v0.1.0 三 lib 首发基础上，补齐 CI 跨平台验证、Kani 证明覆盖、性能基准报告，使 evorule 仓进入稳定的质量门禁循环。

### 规划项

- [ ] macOS + Linux Gitee Go CI 全绿跑通（跨平台 release 验证 F-1）
- [ ] Kani tier0 12 proof 全部实跑通过（含 verify_path_no_panic 优化方案）
- [ ] Kani tier1 C1-1~C1-4 4 个新增 proof 实跑
- [ ] 第三方代码 review 启动（Circle 2 阶段）
- [ ] 跨平台 demo 视频 / GIF 录制（公开宣传素材）

---

## [0.1.0-internal-baseline] - 2026-07-28

> ⚠️ **本段是 v0.1.0 内部基线记录（2026-07-28）。**
> 2026-07-30 走神 9 决策后，evorule 仓真发 crates.io，公开发布版见上方 [0.1.0] - 2026-07-30 段。
> 本段保留作决策历史，标题加 `-internal-baseline` 后缀与公发版区分。

项目首次公开版本。EvoRule 是一个只接受和运行 JSON 数据集的反应式执行引擎，采用三层架构（TCB / Reactor / Governance），提供确定性执行、可审计链、时间旅行调试。

**架构原则**：机制与策略分离。核心层（tier0/tier1/tier2）仅包含机制，应用层功能（HTTP API、SSE、Prometheus、认证、具体 I/O Handler）位于 `evorule-application` 仓库。

### 🆕 新增

#### 三层架构（机制层）

- **evorule-tcb** — 纯计算内核，`#![forbid(unsafe_code)]`，零外部依赖
  - 6 个 JsonValue 变体（Null / Bool / Integer / String / Array / Object）
  - 4 个元指令（set / push / branch / io_request）
  - build.rs 编译时门禁（T15 Fact match 白名单检测等）
  - 5 个 Kani proof（4 PASS + 1 TIMEOUT），proptest 属性测试
  - `MAX_TRANSFORM_RULES = 64` 深度限制

- **evorule-reactor** — 反应器主循环
  - drain → stable → block → execute 四阶段事件循环
  - FactsLog（append-only 事实账本）+ WAL（`persistence` feature）+ 因果链
  - C FFI 接口（`ffi` feature）
  - 时间机器：replay / rewind / fork / diff
  - 调试控制：pause / step / break / phase

- **evorule-governance** — 治理层（纯机制，lib crate）
  - SessionManager：多会话生命周期管理
  - Auditor：BLAKE3 哈希链 + 逻辑时钟 + gzip 压缩导出
  - TimeMachine：时间机器（基于 tier1 FactsLog）
  - IoDispatcher + IoHandler trait：I/O 调度框架（接口定义）
  - IoSubscriber：事件订阅机制
  - IoMetrics trait：可观测性接口（由应用层注入实现）
  - RuleValidator：规则静态安全分析（5 项检查）

#### 应用层（evorule-application）

> 以下功能位于 `evorule-application` 仓库，不属于核心层。

- **evorule-server** — HTTP API 服务
  - 40+ JSON HTTP 端点
  - SSE 事件流（心跳 + 空闲超时 + 连接数限制）
  - Bearer Token 认证
  - Prometheus 指标暴露
- **io_handlers** — 具体 I/O Handler 实现
  - DbHandler（SQLite）
  - HttpHandler（reqwest）
  - MemoryHandler

#### 工具与生态

- **evorule-cli** — 命令行工具（核心仓内置，纯核心能力封装）
  - `validate` 子命令：规则静态验证
  - `verify-chain` 子命令：审计链哈希验证
  - `diff` 子命令：时间机器版本对比
- **5 个 validate-\*.ps1 + validate-all.ps1** — SemVer / CHANGELOG / License / Cargo.lock / Tag 校验
- **CI 流水线** — Gitee Go + GitHub Actions（clippy / test / kani / differential）
- **形式化验证白皮书 v3.1** — 七层验证体系 + 属性目录 + 追溯矩阵

### 🔒 安全

- **SECURITY_AUDIT v0.1.0** — P0 全修复，P1 4 项 HIGH 待公网部署前修复
- **THREAT_MODEL.md** — 威胁建模
- **DEPENDENCY_AUDIT v0.1.0** — 核心 crate 零已知漏洞（cargo audit 通过）
- **Bearer Token 认证** — 应用层功能，位于 evorule-server

### 📚 文档

- **README.md** — 快速开始（核心库 + HTTP API 双路径）、架构图、三层详解
- **VERSION_STRATEGY.md** — 生态版本号标准
- **ROADMAP.md** — 公开路线图
- **STATUS.md** — 当前状态与已知限制
- **形式化验证白皮书 v3.1** — `EVORULE_FORMAL_VERIFICATION_PLAN_v3.md`
- **设计原则** — 透明 / 可选 / 可控 / 可回放 / 可审计

### 🔄 变更

- **协议统一为 AGPL-3.0-or-later**
- **所有 .rs 文件加 SPDX header** — 全覆盖
- **H5/H6 架构迁移** — HTTP API、SSE、Prometheus、认证、具体 I/O Handler 从 evorule-governance 迁移到 evorule-application，核心层保持纯净

### 🐛 修复

- **Clippy 警告修复** — 全工作区 `cargo clippy -- -D warnings` 零错误
- **代码格式化** — 统一 Rust 官方格式
- **差分测试修复** — Reactor 运行时 vs Pure Function 一致性验证通过
- **Kani CI 修复** — 升级 Kani 版本，修正 nightly 兼容性

### 已知问题

- 🟡 **Kani 部分 proof TIMEOUT** — 复杂 proof 因 CBMC 状态爆炸超时，由 proptest 兜底
- ⚠️ **跨平台 release 未全面验证** — Windows 开发验证通过，Linux/macOS CI 覆盖
- ❌ **API 稳定承诺仍不提供** — 1.0 之前 API 仍可能变化
- 🟡 **性能基准未建立** — v0.2.0 引入 criterion 性能测试

详见 [STATUS.md](STATUS.md) §"已知问题"。

---

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evo-rule-lab/evorule>

---

**本变更日志采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
