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

## [0.2.3] - 2026-08-10

**CLI 规则加载修复（PATCH）** — 修复 `evorule` CLI 的 `load_rules` 将规则目录内的初始数据文件 `payload.json` 误当作规则加载的问题。生产机制层（tcb / reactor / governance）无 Rust 源代码改动，仅版本同步 bump；核心变更在 CLI。

### 🐛 修复

- **`evorule-cli` `load_rules` 排除保留数据文件 `payload.json`**（[io_util.rs](evorule-cli/src/io_util.rs)）：
  - 若用户在规则目录内放置初始输入 `payload.json`（通常为 `{}` 或无 `type` 字段），此前会被当作规则加载并触发 `missing field: type` 错误
  - 现按约定排除文件名恰好为 `payload.json`（大小写不敏感）的文件，不参与规则加载；新增单测 `test_load_rules_ignores_payload_json`
  - 修复后，规则目录内可安全放置 `payload.json`（与 `--payload-file` 配合），无需强制把数据文件放目录外

### 🔄 变更

- 版本同步 bump 至 0.2.3（workspace 单一真相源 `Cargo.toml` + 各子 crate 依赖版本对齐）
- **发版计划**：Gitee 发布 v0.2.3 时，crates.io 同步发布 evorule-cli v0.2.3（crates.io 当前停在 v0.2.1）

### 向后兼容

- ✅ 规则加载行为：合法规则文件的加载与确定性排序不变
- ✅ fact log 格式不变
- ✅ CLI 命令与输出格式不变（仅修复误加载场景）

---

## [0.2.2] - 2026-08-10

**协议文档修正 + SDK 合规脚本方向反转** — 修正 SDK 许可证在文档中的"MIT 漏网之鱼"。SDK 是 evorule 核心的衍生作品（封装 evorule-server API + 业务语义层 Guard.check），协议必须与核心保持一致。本次 PATCH 不含任何 Rust 源代码改动，生产功能不受影响。

### 🔄 变更

- **`docs/oss_strategy.md` §3 矩阵 + §3.1 + §5 FAQ 修正**：
  - §3 矩阵：TypeScript SDK / Python SDK 两行 License 由 `MIT` → `AGPL-3.0-or-later`，理由改写为"SDK 是核心衍生作品，协议不能自相矛盾；双轨许可兜底"
  - §3.1 标题由"为什么 SDK 用 MIT？" → "为什么 SDK 也用 AGPL？"，整段重写：阐述衍生作品调用链逻辑（核心 → server → SDK），并解释双轨兜底（内部集成不对外 SaaS / 政府学术非营利免费豁免 / 企业闭源 SaaS 走商业许可）
  - §5 FAQ 原"SDK MIT → 不需要开源"→ 3 场景拆分表（内部集成 / SaaS 二选一 / 政府学术免费豁免）

- **`scripts/update-sdk-license.js` 完全重写（方向反转）**：
  - 头部 SPDX `MIT` → `AGPL-3.0-or-later`
  - 不再内嵌 AGPL 文本，改为 `fs.readFileSync` 读取仓根 LICENSE 文件复用（保证与核心 100% 一致）
  - 替换方向反转：原来"匹配旧 AGPL header → 替换为 MIT" → 反转为"匹配旧 MIT header → 替换为 AGPL"
  - 新增 SDK 目录不存在的防御性检查（早期 v0.x SDK 尚未初始化时跳过而非崩溃）
  - 所有 `.py` / `.ts` / `.md` 新 SPDX header 统一为 `AGPL-3.0-or-later`

### 🐛 修复

- **修复 `update-sdk-license.js` 在 SDK 目录不存在时崩溃**：原脚本假定 `sdk/python` 和 `sdk/typescript` 目录已存在，直接 `readdirSync` 导致 `ENOENT` 退出码 1。新增 `fs.existsSync` 防御检查，跳过并打印警告（v0.2.x 阶段 SDK 尚未创建是正常状态，脚本应在 SDK 实际落地后再跑批量替换）

### 🔒 安全

- **堵死 MIT SDK 灰色通道**：MIT 时期 Agent 公司可通过 SDK 绕过核心 AGPL 不付费（SDK MIT 不传染 → 客户代码完全自由 → 卖闭源 SaaS 无义务）。SDK 改为 AGPL 后，对外 SaaS 场景必须二选一：开源 SaaS 应用层 或 购买商业许可。内部集成 / 政府学术非营利不受影响（双轨兜底）

---

## [0.2.1] - 2026-08-05

**v0.2.0 发布后 Kani 验证同步修正** — v0.2.0 发布后发现的 Kani 验证编译错误、文档过时、CI 配置失效等问题在本次 patch 中修复。生产功能不受影响（所有代码修复均被 `#[cfg(kani)]` 门控）。

### 🐛 修复

- **Kani 验证编译错误修复**（不影响生产构建，仅 `#[cfg(kani)]` 模式）：
  - `evorule-tcb/src/executor.rs`：`ManuallyDrop<Vec<JsonValue>>` 在 Kani 模式下无法直接 `extend`，改用 `inner.iter().cloned()`（`#[cfg(kani)]` 分支）
  - `evorule-reactor/src/facts_log.rs`：`FactsLogLock` 的 `unsafe impl Sync` 与 crate 级 `#![deny(unsafe_code)]` 冲突，添加 `#[allow(unsafe_code)]`
  - `evorule-tcb/verification/kani_proofs.rs`：移除未使用导入 `exec_set`，修正 `FixedMap<4>` slot 数注释
- **examples/tests 版本号文本修正**：`end_to_end.rs` / `integration_end_to_end.rs` / `panic_free.rs` / `proptest_props.rs` 中 `v0.1.0` / `v0.1.0-alpha.1` → `v0.2.0`

### 🔄 变更

- **Cargo.toml 元数据清理**：移除 3 个 crate 中不存在的 `KANI_VERIFICATION_PLAN.md` 从 `exclude` 列表；`evorule-cli` 中 `verify-v0.1.0.sh` → `verify.sh`
- **CI 配置更新**（`.github/workflows/kani.yml`）：标准化 Kani 0.67.0，仅 3 个最简单 proof 入 CI；移除无效 0.14.0 版本与 24h 超时表述
- **PS1 脚本 UTF-8 BOM 恢复**（27 个脚本）：修复 PowerShell 5.1 中文注释乱码导致执行失败
- **Kani 验证实测**（Kani 0.67.0, WSL Ubuntu 22.04, 2026-08-05）：
  - evorule-tcb：12 proof → 9 PASS + 3 TIMEOUT（`evaluate_domain` 系列，proptest 保底）
  - evorule-reactor：11 proof → 10 PASS + 1 TIMEOUT（`invariant_io_count_force_remove`）
- **文档同步**（15+ 文档）：KANI.md / SECURITY_AUDIT / THREAT_MODEL / TCB_SPEC / REACTOR_SPEC 等全面对齐实测结果

### 🗑 弃用

- 删除 `evorule-cli/verify-v0.1.0.sh`（重命名为 `verify.sh`）
- 删除 `evorule-governance/verify-server-v0.1.0.sh`（废弃）

---

## [0.2.0] - 2026-08-04

**evorule-reactor / evorule-governance 重大重构** — `IoType` 从固定 `&'static str` 重构为动态 `Arc<str>`，支持应用层注册任意 io_type；`IoHandler` trait 与 `IoDispatcher` 从 evorule-governance 下沉至 evorule-reactor（机制层基座），解除 agent 对 governance 的依赖。详细迁移步骤见 [MIGRATION_v0.2.0.md](./MIGRATION_v0.2.0.md)。

### ⚠️ Breaking Changes

- **`IoType` 内部表示从 `&'static str` 改为 `Arc<str>`**（`evorule-reactor/src/fact.rs`）
  - 失去 `Copy` trait：所有按值传递处需显式 `.clone()`。典型场景：reactor 中 `state.register_io_request(id, io_type)` 后仍要在 `Fact::IoRequest` 与 `tracing::debug!` 中使用 `io_type`，需 `io_type.clone()`
  - 5 个旧 `const` 改为工厂函数（`Arc::from` 非 const）：
    | v0.1.x（已移除） | v0.2.0（替代） |
    |---|---|
    | `IoType::CALL_EXTERNAL` | `IoType::call_external()` |
    | `IoType::HTTP_GET` | `IoType::http_get()` |
    | `IoType::QUERY_DB` | `IoType::query_db()` |
    | `IoType::SAVE_MEMORY` | `IoType::save_memory()` |
    | `IoType::CALL_SERVICE` | `IoType::call_service()` |
  - 字符串值不变：`IoType::new("call_service") == IoType::call_service()`，旧 WAL / core_eval.json 无需改动
  - `IoType` 仍实现 `Clone + PartialEq + Eq + Hash + PartialOrd + Ord + Send + Sync`，可作 `HashMap`/`BTreeMap` key、跨线程共享、克隆廉价（原子计数）

- **`IoHandler` trait 从 evorule-governance 下沉至 evorule-reactor**（`evorule-reactor/src/io_handler.rs`）
  - 改用 `#[async_trait]` 使其 object-safe，支持 `Arc<dyn IoHandler>` 动态分发
  - `evorule-governance/src/io_handler.rs` 保留 re-export（`pub use evorule_reactor::{IoHandler, IoResult}`），旧 `use evorule_governance::IoHandler` 仍可用
  - 新代码推荐直接 `use evorule_reactor::IoHandler`

- **`IoDispatcher` 从 evorule-governance 下沉至 evorule-reactor**（`evorule-reactor/src/io_dispatcher.rs`）
  - 下沉动机：agent（解决方案1）不依赖 governance，v0.1.x 只能借道 `call_service` 二级路由；v0.2.0 agent 可直接按 IoType 注册 handler
  - reactor 新增 `IoDispatcher::contains()` / `known_types()` 方法，供加载期校验
  - `evorule-governance/src/io_dispatcher.rs` 改为 re-export（消除 v0.1.x 遗留的重复实现），旧 `use evorule_governance::IoDispatcher` 仍可用且自动获得 `contains()` / `known_types()` 新方法

- **`IoType::parse()` 行为变更** — 从"未知返回 None"变为"始终返回 Some"（无条件接受），校验责任移到 subscriber / `ReactorBuilder::known_io_types`。标记 `#[deprecated]`，新代码用 `IoType::new()`

### 🆕 新增

- **`IoType::new(name: &str) -> IoType`** — 运行时构造任意 io_type（v0.2.0 自定义 IoType 入口）。应用层可注册 `IoType::new("retrieve")` / `IoType::new("file")` / `IoType::new("http")` 等自定义类型，无需修改核心宪法
- **`ReactorBuilder::known_io_types(types)`** — 可选快速失败校验（`evorule-reactor/src/reactor.rs`）。注册后，IoRequest 时若 io_type 不在集合内，立即发射 `Fact::Error`（恢复 v0.1.x 拼错 io_type 快速失败的确定性）。未注册（默认）则透传不校验，由 subscriber 决定能否处理。通常从 `IoDispatcher::known_types()` 收集
- **`IoDispatcher::contains(io_type) -> bool`** — 供加载期校验使用
- **`IoDispatcher::known_types() -> impl Iterator<Item = &IoType>`** — 已注册的所有 IoType，供 `known_io_types` 收集

### 🗑 弃用

- **`IoType::parse(s)`** — v0.2.0 起用 `IoType::new`；parse 不再校验，保留仅为向后兼容

### 🔄 变更

- **evorule-reactor 新增 `async-trait` 依赖** — IoHandler trait object-safety 所需（零成本宏，运行时仅一次 `Box<dyn Future>` 分配）
- **evorule-reactor `lib.rs` 公开导出** `IoDispatcher` / `IoDispatcherBuilder` / `IoHandler` / `IoResult`（H5 下沉后从 governance 迁入）
- **evorule-governance `lib.rs`** 保留 `IoDispatcher` / `IoHandler` / `IoResult` 的 re-export（向后兼容）
- **evorule-cli `executor.rs`** — 改用 `IoType::new(&io_type)` 构造（v0.2.0 透传不校验）

### 向后兼容

- ✅ **5 个旧 io_type 字符串值不变** — `IoType::new("call_service") == IoType::call_service()`，HashMap/BTreeMap key 一致
- ✅ **旧 WAL 无需迁移** — io_type 以字符串序列化/反序列化（`wal.rs` 用 `IoType::new(io_type_str)` 反序列化）
- ✅ **core_eval.json 无需改动** — io_type 字段仍是字符串
- ✅ **governance re-export 保留** — `use evorule_governance::{IoHandler, IoDispatcher}` 仍可用
- ✅ **`Fact::IoRequest` 结构不变** — `io_type: IoType` 字段类型名不变，仅内部表示改变

### 本仓内部影响

| 本仓 crate             | 影响说明                                                                                                 |
| ---------------------- | -------------------------------------------------------------------------------------------------------- |
| **evorule-cli**        | `executor.rs` 改用 `IoType::new(&io_type)` 构造（v0.2.0 透传不校验，无 handler 时发 `Fact::Error` 退出） |
| **evorule-tcb**        | 无代码改动（仅版本同步 bump 至 `0.2.0`）                                                                 |
| **evorule-reactor**    | `IoType` 重构 + `IoHandler`/`IoDispatcher` 下沉至本 crate（核心变更主体）                                |
| **evorule-governance** | `io_handler.rs` / `io_dispatcher.rs` 改为 re-export reactor（消除 v0.1.x 遗留重复实现）                  |

> 各独立下游仓的迁移状态由各仓自行记录，不在本仓文档中讨论（各仓独立发布原则）。

### ✅ 验证

- `cargo build --workspace` 通过（evorule-tcb / evorule-reactor / evorule-governance / evorule-cli）
- `cargo test --workspace` 通过（含 `io_dispatcher.rs` 4 个新测试：dispatcher_routes_by_io_type / dispatch_hit_and_miss / new_equals_factory_key_collision + fact.rs io_type roundtrip/parse/new 测试）
- `cargo clippy --workspace --all-targets -- -D warnings` 通过

### 📦 发布

- ✅ **已发布到 crates.io** — `evorule-tcb` / `evorule-reactor` / `evorule-governance` / `evorule-cli` v0.2.0 均已 `cargo publish` 成功

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

- **`reactive_researcher`** repository URL 统一为带连字符版（`gitee.com/evo-rule-lab/evorule`）
- **kani_proofs 编译目标** — 改名时 Kani metadata 列表同步更新

### 🔒 安全

- 升级到 Kani 0.67.0 + nightly-2025-11-21 后，tier0 12 个 Kani proof 实测 9 PASS + 3 TIMEOUT
  - 9 PASS: `verify_value_roundtrip` / `verify_path_no_panic` / `verify_set_integer_safety` / `verify_set_sub_safety` / `verify_jsonvalue_array_safety` / `verify_resolve_path_object_kani` / `verify_execute_transition_kani` / `verify_termination_kani` / `verify_depth_enforcement_kani`（3-231s）
  - 3 TIMEOUT: `verify_evaluate_domain_eq_kani` / `_lt_kani` / `_exists_kani`（CBMC 对嵌套 FixedMap 状态爆炸, 600s 超时, 由 19 个 proptest 保底覆盖）
- tier0 编译时门禁：`#![deny(unwrap_used)]` / `#![deny(expect_used)]` / `#![deny(indexing_slicing)]` / `#![deny(panic)]`
- tier0/1/2 编译时 build.rs 门禁全开（T1/T2/T3/T15）

### 📚 文档

- 「v0.1.0 不发」决策撤销（决策作废，详见撤销说明）
- v0.2.0 本仓规划（独立 release-blocker 拆分多份）
- 发布程序备查文档新增 v1.1（crate 改名 + 元数据 + 阻塞型坑 B1-B8 + 非阻塞 N1-N6）
- evorule 发布准备方案、确定性测试方案 — 上游决策记录

### ✅ 验证

- `cargo build --workspace` exit 0
- `cargo test --workspace` exit 0（643 passed, 0 failed, 4 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings` exit 0
- `cargo publish -p evorule-tcb --dry-run --allow-dirty` exit 0（Packaged 28 files, 539.3KiB / 112.5KiB compressed）

### 已知问题

- ⚠️ **v0.1.0 跨平台 release 实测** 仅 Windows + WSL，macOS 待 CI 跑过确认
- ⚠️ **Kani tier0 12 proof**：实测 9 PASS + 3 TIMEOUT（`evaluate_domain` 系列, proptest 保底）
- ⚠️ **Kani tier1 11 proof**：实测 10 PASS + 1 TIMEOUT（`invariant_io_count_force_remove`, BTreeSet 状态爆炸）
- ⚠️ **跨平台 release 验证 (F-1)** 状态 3 个 ❌ 待改 ✅
- ⚠️ **Gitee Go CI 真实跑通 (F-2)** 需 push 后才能看到 run ID
- ⚠️ **5 终极门禁** 需打 v0.1.0 tag 前全过

---

### 形式化验证（A-1 / A-2 / C-1）

- **evorule-tcb 12 个 Kani proof**（A-1）— WSL Kani 0.67.0 + CBMC 实跑：**9 PASS + 3 TIMEOUT**
  - `verify_value_roundtrip` / `verify_path_no_panic` / `verify_set_integer_safety` / `verify_jsonvalue_array_safety` / `verify_set_sub_safety` / `verify_resolve_path_object_kani` / `verify_evaluate_domain_eq_kani` / `verify_evaluate_domain_lt_kani` / `verify_evaluate_domain_exists_kani` / `verify_execute_transition_kani` / `verify_termination_kani` / `verify_depth_enforcement_kani`
  - TIMEOUT 的 3 个 `evaluate_domain` 系列由 19 个 proptest 属性测试保底覆盖
- **evorule-reactor 11 个 Kani proof**（A-2，7 原有 + 4 新 C1-1~C1-4）— 实跑：**10 PASS + 1 TIMEOUT**
  - 7 原有：`invariant_io_count_register_complete` / `invariant_io_count_force_remove` / `invariant_version_monotonic` / `invariant_io_recovery_iff_result` / `command_does_not_decrease_queue` / `max_rounds_termination` / `invariant_cause_queue_sync`
  - 4 新：`proof_fact_log_append_monotonic` / `proof_hash_chain_back_link` / `proof_reactor_invariants_preserved_after_pure_ops` / `proof_phase_state_machine_cannot_jump`
  - TIMEOUT 的 `invariant_io_count_force_remove`（BTreeSet force_remove 状态爆炸）由 proptest 保底
- **C-1 `verify_path_no_panic` Kani 真实通过** — 原 BTreeMap 路径爆炸问题通过基于 Vec 的 KIdSet/KIdMap 替代方案解决

### 引擎质量（A-3 / B-1 / B-2 / E-1）

- **A-3 fact log 索引 + 长期压缩** — 3 个 BTreeMap 索引（version_index / fact_id_index / path_index）加速查询；手动 `compact()` 快照压缩，5000 facts 压缩率 99.98%（>> 40% 阈值）
- **B-1 clippy 0 warnings** — `cargo clippy --workspace --all-targets -- -D warnings` exit=0
- **B-2 cargo audit 0 CVE** — 公网 RustSec DB 实跑 0 CVE
- **E-1 criterion 性能基准** — evorule-tcb 3 组基准（execute_transition / jsonvalue_ops / path_resolve）实跑

### 机制-应用边界清理

- **evorule 仓独立 release** — 核心层仅包含机制；HTTP API、I/O Handler 实现、可视化、业务模板等应用层功能不在本仓
- **evorule-governance 精简为纯机制层库** — IoDispatcher 框架 + IoHandler trait re-export（具体 I/O Handler 实现不在本仓）
- **README 定位调整** — 叙事调整为"通用反应式执行引擎"
- **SDK 不在本仓** — 多语言客户端 SDK 见 evorule-sdk 独立仓
- **evorule-cli 业务规则模板外移**（2026-07-30）— `evorule-cli/examples/hospital/` + `evorule-cli/examples/law-firm/` 两套行业规则集（HIPAA / 律所合规）共 10 个文件，按 `AGENTS.md` 边界判断表「evorule-cli 加规则模板 = 业务内容，不是机制，放应用层 ❌」迁出本仓；evorule 核心仓 `evorule-cli/examples/` 仅保留机制层最小化演示 README 与 tests/fixtures 用例
  - 【留痕声明】本次迁移是**经项目方明确决策并确认**的越界清理操作，遵循 AGENTS.md 规则二的警告确认流程，不再回迁

### 🗑 移除（H5 边界清理）

- **移除 portal** — 聚合端点属应用层 UI
- **移除 hot_reload** — 业务规则热重载属策略层，移除 `notify` 依赖
- **移除 cluster** — 多反应器协作原语属高级功能，后续在 application 仓实现
- **移除 object_pool** — FactsLog 对象复用优化属性能优化，待后续评估重加
- **metrics / auth 转为 feature flag** — 默认不编译，减少嵌入式场景依赖

### 🐛 修复

- **MSRV** — `is_multiple_of` → `%` 取模（reactor.rs / auditor.rs），兼容 Rust 1.74
- **时间机器 diff 递归** — 从顶层字段对比扩展为嵌套对象递归对比
- **Kani 路径爆炸** — BTreeSet/BTreeMap → KIdSet/KIdMap（Vec 基实现），reactor 11 个 proof 实测 10 PASS + 1 TIMEOUT（`invariant_io_count_force_remove` 仍超时）

### 已知问题（v0.1.0 首发时仍存在）

- ⚠️ **P1 安全修复待公网部署前完成** — H6 SSRF / H7 SQL / H8 CORS / H9 DB URL（具体 I/O Handler 实现位于应用层，不在本仓）
- ⚠️ **跨平台 release 实测** 仅 Windows + WSL，macOS 待 CI 跑过确认
- ❌ **API 稳定承诺仍不提供** — 1.0 之前 API 仍可能变化

---

## [0.1.0-internal-baseline] - 2026-07-28

> ⚠️ **本段是 v0.1.0 内部基线记录（2026-07-28）。**
> 2026-07-30 走神 9 决策后，evorule 仓真发 crates.io，公开发布版见上方 [0.1.0] - 2026-07-30 段。
> 本段保留作决策历史，标题加 `-internal-baseline` 后缀与公发版区分。

项目首次公开版本。EvoRule 是一个只接受和运行 JSON 数据集的反应式执行引擎，采用三层架构（TCB / Reactor / Governance），提供确定性执行、可审计链、时间旅行调试。

**架构原则**：机制与策略分离。核心层（tier0/tier1/tier2）仅包含机制，应用层功能（HTTP API、SSE、Prometheus、认证、具体 I/O Handler）不在本仓。

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

> 应用层功能（HTTP API 服务、具体 I/O Handler 实现等）不在本仓。

#### 工具与生态

- **evorule-cli** — 命令行工具（核心仓内置，纯核心能力封装）
  - `validate` 子命令：规则静态验证
  - `verify-chain` 子命令：审计链哈希验证
  - `diff` 子命令：时间机器版本对比
- **5 个 validate-\*.ps1 + validate-all.ps1** — SemVer / CHANGELOG / License / Cargo.lock / Tag 校验
- **CI 流水线** — Gitee Go + GitHub Actions（clippy / test / kani / differential）
- **形式化验证白皮书** — 七层验证体系 + 属性目录 + 追溯矩阵

### 🔒 安全

- **SECURITY_AUDIT v0.1.0** — P0 全修复，P1 4 项 HIGH 待公网部署前修复
- **THREAT_MODEL.md** — 威胁建模
- **DEPENDENCY_AUDIT v0.1.0** — 核心 crate 零已知漏洞（cargo audit 通过）
- **Bearer Token 认证** — 应用层功能，不在本仓

### 📚 文档

- **README.md** — 快速开始（核心库 + HTTP API 双路径）、架构图、三层详解
- **VERSION_STRATEGY.md** — 生态版本号标准
- **形式化验证白皮书** — `EVORULE_FORMAL_VERIFICATION_PLAN_v3.md`
- **设计原则** — 透明 / 可选 / 可控 / 可回放 / 可审计

### 🔄 变更

- **协议统一为 AGPL-3.0-or-later**
- **所有 .rs 文件加 SPDX header** — 全覆盖
- **H5/H6 架构清理** — HTTP API、SSE、Prometheus、认证、具体 I/O Handler 不在 evorule 核心仓内，核心层保持纯净

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

---

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evo-rule-lab/evorule>

---

**本变更日志采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
