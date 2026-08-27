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
- ❌ 移除
- ⚠️ Breaking Change
- 🔒 安全

---

## [Unreleased]

### 🔄 变更

- **T8 核心仓最小化：ReAct 应用剧本整体迁出至消费方**(依据 [system-rules T8 调查报告],2026-08-27 定调方案 A"应用自持运行宪法"):
  - `evorule-tcb/core_eval.json` v0.3.1 → **v0.4.0**:移除三条 ReAct 循环规则(约全文 54%),回归最小引擎自评估集(increment/decrement/set/sequence/conditional/while_loop/noop/兜底);经 rule_set v1.0 门禁校验
  - **机制零改动**:6 元指令白名单、9 指令类型(call_external 等)、has_fields/collect/merge 语言层能力全部保留——迁出的是剧本不是语言
  - `transition.rs` 测试去 ReAct 化改名:`react_e2e_tests` → `io_loop_e2e_tests`(断言零改动)
  - `reactive_researcher` 示例自带 `assets/constitution.json`(app.evorule.example.researcher v0.4.0),CLI 参数 `--core-eval`/`EVORULE_CORE_EVAL` 改为 `--constitution`/`EVORULE_CONSTITUTION`,解除对核心仓资产的跨路径加载
  - 消费方范式:evo-agent 已自持 `app.evoagent.agent` v0.4.0(evo-agent 仓先行提交 `0845282`)
  - 迁移指引:凡依赖 org.evorule.core.eval v0.3.1 中 call_external/call_service 规则的部署,请改用消费方自持的运行宪法(副作用:未知指令交兜底规则处理,不再产生 LLM 循环)

### ❌ 移除

- **T7 核心仓瘦身：一次性战地脚本退场（25 个）**(依据 [system-rules T7 调查报告],2026-08-27 定调"四 crate 内核论"):
  - markdownlint 清洗群(15):`fix-md040{,.js,-v2}.ps1`、`show-{errors,md013*,md037,md051}`、`list-md013`、`find-md040`、`count-errors`、`show-remaining`、`fix-corrupted-{lines,newlines,quotes}`
  - SPDX 头灌装群(4):`add-spdx-ffi`、`add-spdx-safe`、`add_spdx_headers`、`update-spdx.js`(头部已全线就位)
  - 迁移/调试残留(4):`migrate-cli-examples-to-application.{ps1,sh}`、`agents-md-to-schema.py`(输入 AGENTS.md 已不存在)、`test-api-with-hash-diagnosis.ps1`
  - 历史演变工具(2):`update-sdk-license.js`(许可证格局已定型)、`start-server.ps1`(零引用零文档,2026-08-27 明示批准删除)
  - 全部经全仓调用方取证为零存活引用;生产链路零依赖;git 历史可考古
- **保留判据入档**:evorule 仓 = 四 crate(tcb/reactor/governance/cli) + 支撑测试验证 CI 门禁 + 对外契约文档;新增 `scripts/` 文件须能回答"谁还在用它"。

## [0.3.2] - 2026-08-26

**API 正确性修正 + 门禁治理增强 + 新模块 + G-A1 审计锚点签名** — 删除"假验证"陷阱函数 `verify_hash_chain`；审计/会话 API 从静默退化 `"{}"` 改为显式 `Result`；元指令白名单对齐 TCB 6 元指令（移除误混的指令层类型）；build.rs 新增 L2 变更治理门禁与策略层反模式检测；governance 新增 permission 模块、reactor 新增 io_context 模块；新增 G-A1 审计锚点签名（ed25519 确定性签名，真实性/防抵赖）。

### ⚠️ Breaking Changes

- **`verify_hash_chain` 函数彻底删除**(`evorule-governance/src/hash.rs` / `evorule-cli/src/hash.rs`):
  - 原函数始终返回 `true`,是"假验证"陷阱(仅自洽重算链式哈希,不验证已存储的哈希)
  - v0.2.0 起标记为 `#[deprecated]`,v0.3.2 彻底删除
  - 替代方案:用 `compute_chain_hash` 重算后与存储的链哈希比对;CLI 用户用 `verify-chain` 命令读取带哈希字段的 WAL 并逐一校验
- **`auditor.report()` / `auditor.export()` 返回值变更**(`evorule-governance/src/auditor.rs`):
  - 从 `String` 改为 `Result<String, serde_json::Error>`
  - 不再静默退化为 `"{}"`(防止审计数据被误判为"空")
  - 调用方必须处理 `Result`
- **`session.audit_report()` / `session.audit_export()` 返回值变更**(`evorule-governance/src/session.rs`):
  - 从 `String` 改为 `Result<String, String>`
  - mutex 中毒或序列化失败时返回 `Err`,不再静默退化为 `"{}"`

### 🔄 变更

- **元指令白名单修正**(`evorule-governance/src/rule_validation.rs` / `evorule-cli/src/commands/validate.rs`):
  - 移除 `noop` / `increment` / `decrement`(这些是**指令层**类型,不是元指令层,之前误混导致假阳性/假阴性)
  - 新增 `collect` / `merge`(对齐 TCB `executor.rs::execute_meta_instruction` 的 6 种元指令:branch / set / push / io_request / collect / merge)
  - 影响:`evorule validate` 对包含 noop/increment/decrement 作为 transform type 的规则会报错
- **`MAX_NESTING_DEPTH` 从 8 改为 64**(`evorule-governance/src/rule_validation.rs`):
  - 对齐 TCB `MAX_BRANCH_DEPTH=64`,避免校验器比引擎更严造成假阳性(SSOT:同一上限不得多处不同定义)
- **`set` 非法 operation 提升为 error**(`evorule-governance/src/rule_validation.rs`):
  - set 的 operation 不在合法值(set/add/sub)中时,从 warn 提升为 error 阻断
  - 原因:TCB `exec_set` 对未知 operation 硬失败 `UnknownOperation`,校验层不得降级为 warn
- **`merge` 新增 `tool_result` / `tool_results` 校验**(`evorule-governance/src/rule_validation.rs`):
  - merge 必须声明 `tool_result` 或 `tool_results` 之一,否则报错
  - 原因:引擎 `exec_merge` 缺任一报 `MissingField`
- **以点号结尾的路径返回 `None`**(`evorule-tcb/src/path.rs`):
  - 如 `"x."` 现在被视为非法语法返回 `None`,之前会被静默接受为前缀路径
  - 原因:点号是分隔符,必须有后续段;不检查会掩盖拼写错误
- **Fact 类型映射修正**(`evorule-governance/src/auditor.rs`):
  - 移除 `ControlSignal`(Fact 枚举无此变体)
  - 新增 `Stable`(终止事实,WAL 重载之前被映射为 Unknown、丢失类型语义)
  - `Unknown` 保留作导入 fallback 表项

### 🆕 新增

- **governance `permission` 模块**(`evorule-governance/src/lib.rs`):
  - 新增公开导出:`ConditionEvaluator` / `DefaultPolicy` / `PermissionEntry` / `PermissionError` / `PermissionGate` / `PermissionState` / `PermissionTable` / `Verdict`
- **reactor `io_context` 模块**(`evorule-reactor/src/lib.rs`):
  - 新增公开导出:`CallerRole` / `IoCallContext` / `CallerRoleResolver`
- **G-A1 审计锚点签名**(`evorule-governance/src/signing.rs` / `evorule-cli/src/signing.rs`):
  - governance 新增 `signing` 模块 + 公开导出 `AuditSigner` / `SignError` / `verify_signature`
  - CLI 新增 `anchor-keygen`（生成 ed25519 密钥对）与 `verify-anchors`（离线校验审计锚点真实性）子命令
  - ed25519（RFC 8032）确定性签名（无 RNG），与确定性执行纪律兼容；私钥不落盘审计资产/规则库，由调用方注入 32 字节种子
- **`core_eval.json` 新增元数据字段**(`evorule-tcb/core_eval.json`):
  - 新增 `$schema` / `kind` / `id` 字段,规范化规则集格式

### 🔒 安全 / 门禁

- **build.rs 新增 L2 变更治理门禁**(evorule-tcb / evorule-reactor / evorule-governance / evorule-cli):
  - `CHANGE_REQUEST.md` 必须存在且包含所有必填字段
  - 审查状态必须为"已批准"或"紧急通过",否则构建失败
  - 新增环境变量 `EVORULE_SKIP_CR_GATE=1` 可跳过(仅限本地开发,跳过必须临时且有书面理由)
- **build.rs 新增策略层反模式检测**:
  - 扫描 `src/` 目录(自动剥离 `mod tests` 块),禁止策略层代码(conditional / while_loop / sequence 等控制流指令)进入机制层
  - 检测到违规时构建失败,包含详细违规信息
- **三仓 build.rs 门禁实现同步**:
  - `evorule-tcb` / `evorule-reactor` / `evorule-governance` 的 build.rs 保持同一份内联副本实现
  - 任何对门禁逻辑的修改必须三仓同步,防止三个核心模块的审查标准走偏

### 🧪 测试

- **`clock.rs` 新增单元测试**(`evorule-governance/src/clock.rs`):
  - `new_starts_at_zero` / `tick_is_monotonic` / `merge_takes_max_plus_one`
  - doctest 改为 `no_run`(Windows 杀软/WDAC 会间歇性拦截批量 `cargo test --doc` 时刚编译的 doctest 二进制,为保证测试确定性不在此执行)

### ✅ 向后兼容

- ❌ 不兼容:见上方 Breaking Changes(3 项)
- ✅ fact log 格式不变
- ✅ TCB 核心执行逻辑不变(仅 path.rs 边界行为收紧)
- ✅ CLI 命令列表不变(仅 validate 行为变更)

---

## [0.3.1] - 2026-08-18

**TCB Ignored 变体 + I/O 结果按类型隔离 + ReAct 循环完整支持** — 机制层从"静默失败"转"显式 Error 事实"；I/O 结果从全局 `__io_result__` 改为按 io_type 隔离；ReAct 循环补完整支持。

> 注: 0.3.0 内部未独立发布,本次合并为 v0.3.1 单版本。

### ⚠️ Breaking Changes

- 机制层 payload 字段名变化: `__io_result__` → `__io_results__.{io_type}`,core_eval.json 规则引用路径需更新
- `call_external` 元指令参数从 `prompt` 改为 `messages`(LLM 消息历史数组格式),prompt 字段被忽略而非报错
- 未知指令不再静默当 noop,改为产生 Error 事实(影响 core_eval.json 假设"未匹配类型 = noop"的规则)

### 🆕 新增

- **TCB `TransitionResult::Ignored` 变体**: 显式暴露"指令被忽略"(无匹配 transform 规则或规则产生 noop 效果),上层产生 Error 事实使系统显式感知,不再静默失败
- **ReAct 循环完整支持**:
  - `call_external`: 用 `messages`(LLM 消息历史数组) + `tools` 替代旧 `prompt`
  - 新增 `collect` 元指令(多工具扇出:从 `tool_calls` 生成 `call_service` 指令)
  - 新增 `merge` 元指令(消息历史合并 + push 下一次 `call_external` 推进循环)
- **新域类型 `has_fields`**: 检测对象字段集合,空 / null 数组视为 false;ReAct 循环 `if has_fields(tool_calls) then collect else io_request` 分支用
- **形式化验证 Kani proofs 重建**: 34 个 `#[kani::proof]`,5 层覆盖(基础类型/路径解析/域评估/元指令/状态转换),覆盖 P0-P21 关键不变式,接入 CI

### 🔄 变更

- **I/O 结果按类型隔离** (`__io_result__` → `__io_results__.{io_type}`): 旧版单一字段会被任意 io_type 消费,可能让后续不同 io_type 指令错误走 on_true 消费旧值;v0.3.1 按 io_type 隔离,恢复执行后整体清除,杜绝残留
- **`all([])` 兜底规则不再视为业务规则**: 未知指令不再静默当 noop,返回 `Ignored` → Error
- **死循环修复**: null 注入不再触发 I/O 恢复执行;error 响应也正确处理,不再重新推送原指令

### 🐛 修复

- **连续 I/O 调用残留 bug**: `call_external` → 不同 io_type 指令,旧 `__io_result__` 残留导致第二次错误消费旧值(已通过 `__io_results__` 隔离根治)
- **null / error 响应死循环**: 见上方"死循环修复"
- **Version 一致性**: Error 事实不 bump log version(原始设计)但 reactor 之前会 bump(不一致),已对齐
- **2 个陈旧测试与 v0.3.1 设计对齐**

### 📚 文档

- **2 份新 examples**:
  - `evorule-tcb/examples/basic_transition.rs` — 5 步上手核心 API `execute_transition`
  - `evorule-cli/examples/programmatic_run.rs` — 把 `evorule` 当 library 嵌入 Rust 应用
- doc-tests 补全(`rule_validation` / `CliError::exit_code` / `fact_to_human`)

### ✅ 向后兼容

- ✅ TCB / Reactor / Governance 公开 Rust API 兼容(仅新增 `TransitionResult::Ignored` 变体,不删任何项)
- ✅ fact log 格式不变
- ✅ CLI 命令与输出格式不变

---

## [0.2.4] - 2026-08-15

**形式化验证 P0 阶段完善（PATCH）** — 落地差分测试语义等价规约、evaluate_domain 分层 Kani harness、验证证据归档规范。机制层无 API 破坏性变更。

### 🆕 新增

- **验证证据归档规范**: 自动化收集验证过程关键元数据(提交 SHA / 工具链版本 / 测试日志 / 差分结果),证据按 `evorule-{crate}/verification/evidence/` 归档,保证验证结果可追溯、可复核
- **差分测试语义等价规约**: Reactor 异步流水线与 `execute_transition` 纯函数结果升级为全 payload 比较,差分测试实跑并归档 PASS 证据

### 🔄 变更

- **evaluate_domain 分层 Kani harness**: 移除原超时 proof(3 层嵌套 FixedMap 导致 CBMC 状态爆炸),拆分为原子层(eq / lt / exists)+ 组合层(and / not)共 5 个 proof,降低状态空间、提升验证效率

### ✅ 向后兼容

- ✅ 机制层公开 API 不变(tcb / reactor / governance)
- ✅ fact log 格式不变
- ✅ CLI 命令与输出格式不变

---

## [0.2.3] - 2026-08-10

**CLI 规则加载修复（PATCH）** — 修复 `evorule` CLI 的 `load_rules` 将规则目录内的初始数据文件 `payload.json` 误当作规则加载的问题。生产机制层无 Rust 源代码改动。

### 🐛 修复

- **`evorule-cli` `load_rules` 排除保留数据文件 `payload.json`**: 若用户在规则目录内放置初始输入 `payload.json`,此前会被当作规则加载并触发 `missing field: type` 错误;现按约定排除文件名恰好为 `payload.json`(大小写不敏感)的文件,规则目录内可安全放置初始数据文件

### ✅ 向后兼容

- ✅ 合法规则文件的加载与确定性排序不变
- ✅ fact log 格式不变
- ✅ CLI 命令与输出格式不变(仅修复误加载场景)

---

## [0.2.2] - 2026-08-10

**协议文档修正 + SDK 合规脚本方向反转** — 修正 SDK 许可证在文档中的"MIT 漏网之鱼"。SDK 是 evorule 核心的衍生作品,协议必须与核心保持一致。本次 PATCH 不含任何 Rust 源代码改动。

### 🔄 变更

- **SDK 许可证策略修正**: TypeScript SDK / Python SDK 由 `MIT` → `AGPL-3.0-or-later`(SDK 是核心衍生作品,协议不能自相矛盾);双轨许可兜底(内部集成不对外 SaaS / 政府学术非营利免费豁免 / 企业闭源 SaaS 走商业许可)
- **`scripts/update-sdk-license.js` 方向反转**: 从"匹配旧 AGPL header → 替换为 MIT"反转为"匹配旧 MIT header → 替换为 AGPL";新增 SDK 目录不存在的防御性检查

### 🐛 修复

- **`update-sdk-license.js` 在 SDK 目录不存在时崩溃**: 原脚本假定 SDK 目录已存在,直接 `readdirSync` 导致 `ENOENT`;新增 `fs.existsSync` 防御检查,跳过并打印警告

### 🔒 安全

- **堵死 MIT SDK 灰色通道**: MIT 时期可通过 SDK 绕过核心 AGPL 不付费;SDK 改为 AGPL 后,对外 SaaS 场景必须二选一:开源 SaaS 应用层 或 购买商业许可。内部集成 / 政府学术非营利不受影响

---

## [0.2.1] - 2026-08-05

**v0.2.0 发布后 Kani 验证同步修正（PATCH）** — v0.2.0 发布后发现的 Kani 验证编译错误、文档过时、CI 配置失效等问题修复。生产功能不受影响(所有代码修复均被 `#[cfg(kani)]` 门控)。

### 🐛 修复

- **Kani 验证编译错误修复**(不影响生产构建,仅 `#[cfg(kani)]` 模式):
  - `executor.rs`: `ManuallyDrop<Vec<JsonValue>>` 在 Kani 模式下无法直接 `extend`,改用 `iter().cloned()`
  - `facts_log.rs`: `FactsLogLock` 的 `unsafe impl Sync` 与 crate 级 `#![deny(unsafe_code)]` 冲突,添加 `#[allow(unsafe_code)]`
  - `kani_proofs.rs`: 移除未使用导入,修正 slot 数注释
- **examples/tests 版本号文本修正**: `v0.1.0` / `v0.1.0-alpha.1` → `v0.2.0`

### 🔄 变更

- **Cargo.toml 元数据清理**: 移除不存在的文件从 `exclude` 列表;脚本路径修正
- **CI 配置更新**: 标准化 Kani 0.67.0,仅最简单 proof 入 CI;移除无效版本与超时表述
- **PS1 脚本 UTF-8 BOM 恢复**(27 个脚本): 修复 PowerShell 5.1 中文注释乱码导致执行失败
- **Kani 验证实测**(Kani 0.67.0): evorule-tcb 12 proof → 9 PASS + 3 TIMEOUT(proptest 保底);evorule-reactor 11 proof → 10 PASS + 1 TIMEOUT

### 🗑 弃用

- 删除 `evorule-cli/verify-v0.1.0.sh`(重命名为 `verify.sh`)
- 删除 `evorule-governance/verify-server-v0.1.0.sh`(废弃)

---

## [0.2.0] - 2026-08-04

**evorule-reactor / evorule-governance 重大重构** — `IoType` 从固定 `&'static str` 重构为动态 `Arc<str>`,支持应用层注册任意 io_type;`IoHandler` trait 与 `IoDispatcher` 从 evorule-governance 下沉至 evorule-reactor(机制层基座),解除 agent 对 governance 的依赖。

### ⚠️ Breaking Changes

- **`IoType` 内部表示从 `&'static str` 改为 `Arc<str>`**(`evorule-reactor/src/fact.rs`):
  - 失去 `Copy` trait:所有按值传递处需显式 `.clone()`
  - 5 个旧 `const` 改为工厂函数(`IoType::CALL_EXTERNAL` → `IoType::call_external()` 等)
  - 字符串值不变:`IoType::new("call_service") == IoType::call_service()`,旧 WAL / core_eval.json 无需改动
- **`IoHandler` trait 从 evorule-governance 下沉至 evorule-reactor**: 改用 `#[async_trait]` 使其 object-safe,支持 `Arc<dyn IoHandler>` 动态分发;governance 保留 re-export,旧 `use evorule_governance::IoHandler` 仍可用
- **`IoDispatcher` 从 evorule-governance 下沉至 evorule-reactor**: agent 可直接按 IoType 注册 handler,无需借道 `call_service` 二级路由;governance 改为 re-export(消除重复实现)
- **`IoType::parse()` 行为变更**: 从"未知返回 None"变为"始终返回 Some"(无条件接受),校验责任移到 subscriber;标记 `#[deprecated]`,新代码用 `IoType::new()`

### 🆕 新增

- **`IoType::new(name: &str) -> IoType`**: 运行时构造任意 io_type(v0.2.0 自定义 IoType 入口),应用层可注册自定义类型无需修改核心宪法
- **`ReactorBuilder::known_io_types(types)`**: 可选快速失败校验,注册后 IoRequest 时若 io_type 不在集合内立即发射 `Fact::Error`;未注册(默认)则透传不校验
- **`IoDispatcher::contains(io_type) -> bool`**: 供加载期校验使用
- **`IoDispatcher::known_types() -> impl Iterator<Item = &IoType>`**: 已注册的所有 IoType,供 `known_io_types` 收集

### 🗑 弃用

- **`IoType::parse(s)`**: v0.2.0 起用 `IoType::new`;parse 不再校验,保留仅为向后兼容

### 🔄 变更

- evorule-reactor 新增 `async-trait` 依赖(IoHandler trait object-safety 所需)
- evorule-reactor `lib.rs` 公开导出 `IoDispatcher` / `IoHandler` / `IoResult`
- evorule-governance `lib.rs` 保留 re-export(向后兼容)
- evorule-cli `executor.rs` 改用 `IoType::new(&io_type)` 构造

### ✅ 向后兼容

- ✅ 5 个旧 io_type 字符串值不变,HashMap/BTreeMap key 一致
- ✅ 旧 WAL 无需迁移(io_type 以字符串序列化/反序列化)
- ✅ core_eval.json 无需改动
- ✅ governance re-export 保留
- ✅ `Fact::IoRequest` 结构不变(仅内部表示改变)

---

## [0.1.1] - 2026-08-01

**exec_set 路径解析诊断增强 + 中间节点/空列表语义收紧** — `exec_set` 路径校验加强、中间节点语义收紧、空列表 no-op、指令列表数组展平;附带 evorule-reactor clippy `indexing_slicing` 修复。

### 🐛 修复

- **`exec_set` 路径校验加强**: `parts.first() == Some(&"")` → `parts.iter().any(|p| p.is_empty())`,捕获 `"a..b"` / `"a.b."` 等中间/尾部空段(原仅查首段)
- **`exec_set` 中间节点语义收紧**: null/缺失自动建空对象继续 descend;其他非对象类型(integer/boolean/string/array)返回 `PathResolutionFailed` 且**不覆盖原值**(原对 null 报错、对其他非对象静默覆盖,语义不一致)
- **`exec_push` 空列表改 no-op**: `EmptyInstructionList` 错误 → `Ok(state)`,支持合法的空 `else: []` / `then: []`(变体保留以维持错误类型 API 稳定)
- **`resolve_instructions_list` 数组展平**: 路径引用解析为数组时 `extend` 展平入队,修复 `body`/`then`/`else` 为指令列表时的入队错误
- **evorule-reactor clippy `indexing_slicing` 修复**: `facts_log.rs` / `reactor.rs` 三处索引切片改用 `get().unwrap_or(&[])`,修复 Windows 工具链下 clippy 阻塞

> **行为变更说明**: null 中间节点与空列表 push 从"报错"变为"成功"。这是修正原错误行为(合法输入被拒),非破坏性变更;错误**类型**未变,公开 API 签名未变。

### 🆕 新增

- **`PathResolutionFailed` 诊断消息丰富**: 含四要素:失败路径 + 出问题的段名 + 实际类型 + 期望类型,供上层日志直接 `Display` 输出
- **`IoDispatcher` 实现 `Clone`**: 内部全为 `Arc<dyn IoHandler>`,clone 仅增引用计数
- **`SessionManager` 新增 `core_eval()` getter + `replace_core_eval()` 原子替换**: 仅影响新会话,已运行会话的 TCB 不可变语义不变
- **集成测试 `set_path_diagnostics.rs`**: 8 场景锁定诊断契约

### 📚 文档

- **TCB_SPEC.md 新增 §八「错误语义与路径解析诊断契约」**: 含中间节点自动创建策略表、诊断消息四要素、可执行规约引用

### ✅ 向后兼容

- ✅ 公开 API 签名未变
- ✅ 错误类型未变(`EmptyInstructionList` 变体保留)
- ✅ 上述行为变更是修正原错误行为,非破坏性变更

---

## [0.1.0] - 2026-07-30

**evorule 仓首次公开发布** — 3 个核心 lib crate 改名后发布到 crates.io:`evorule-tcb` / `evorule-reactor` / `evorule-governance`。

### 🆕 新增

#### 三层架构(机制层)

- **evorule-tcb** — 纯计算内核,`#![forbid(unsafe_code)]`,零外部依赖
  - 6 个 JsonValue 变体(Null / Bool / Integer / String / Array / Object)
  - 4 个元指令(set / push / branch / io_request)
  - build.rs 编译时门禁(Fact match 白名单检测等)
  - Kani 形式化验证 + proptest 属性测试
  - `MAX_TRANSFORM_RULES = 64` 深度限制
- **evorule-reactor** — 反应器主循环
  - drain → stable → block → execute 四阶段事件循环
  - FactsLog(append-only 事实账本)+ WAL(`persistence` feature)+ 因果链
  - C FFI 接口(`ffi` feature)
  - 时间机器:replay / rewind / fork / diff
  - 调试控制:pause / step / break / phase
- **evorule-governance** — 治理层(纯机制,lib crate)
  - SessionManager:多会话生命周期管理
  - Auditor:BLAKE3 哈希链 + 逻辑时钟 + gzip 压缩导出
  - TimeMachine:时间机器(基于 tier1 FactsLog)
  - IoDispatcher + IoHandler trait:I/O 调度框架(接口定义)
  - IoSubscriber:事件订阅机制
  - IoMetrics trait:可观测性接口(由应用层注入实现)
  - RuleValidator:规则静态安全分析(5 项检查)

#### 工具与生态

- **evorule-cli** — 命令行工具(`validate` / `verify-chain` / `diff` 子命令)
- **5 个 validate-*.ps1 + validate-all.ps1** — SemVer / CHANGELOG / License / Cargo.lock / Tag 校验
- **CI 流水线** — Gitee Go + GitHub Actions(clippy / test / kani / differential)
- **形式化验证白皮书** — 七层验证体系 + 属性目录 + 追溯矩阵

### 🔄 变更

- **品牌统一改名**: `tier0-tcb` → `evorule-tcb`,`tier1-reactor` → `evorule-reactor`,`tier2-governance` → `evorule-governance`;子 crate 依赖改为 `path + version` 双声明(本地开发 + crates.io 发布两不耽误);MSRV 兼容 Rust 1.74
- **协议统一为 AGPL-3.0-or-later**
- **机制-应用边界清理**: HTTP API、SSE、Prometheus、认证、具体 I/O Handler 实现不在核心仓;portal / hot_reload / cluster / object_pool 移除;metrics / auth 转为 feature flag

### ❌ 移除

- **portal** — 聚合端点属应用层 UI
- **hot_reload** — 业务规则热重载属策略层,移除 `notify` 依赖
- **cluster** — 多反应器协作原语属高级功能,后续在 application 仓实现
- **object_pool** — FactsLog 对象复用优化属性能优化,待后续评估重加

### 🔒 安全

- 编译时门禁:`#![deny(unwrap_used)]` / `#![deny(expect_used)]` / `#![deny(indexing_slicing)]` / `#![deny(panic)]`
- build.rs 编译时门禁全开
- Kani 验证:tcb 12 proof → 9 PASS + 3 TIMEOUT(proptest 保底);reactor 11 proof → 10 PASS + 1 TIMEOUT

### ⚠️ 已知问题

- 跨平台 release 实测仅 Windows + WSL,macOS 待 CI 跑过确认
- Kani 部分 proof TIMEOUT(复杂 proof 因 CBMC 状态爆炸,由 proptest 兜底)
- 1.0 之前 API 仍可能变化,不提供 API 稳定承诺

---

## [0.1.0-internal-baseline] - 2026-07-28

> ⚠️ **本段是 v0.1.0 内部基线记录(2026-07-28)。**
> 2026-07-30 决策后,evorule 仓真发 crates.io,公开发布版见上方 [0.1.0] - 2026-07-30 段。
> 本段保留作决策历史。

项目首次公开版本。EvoRule 是一个只接受和运行 JSON 数据集的反应式执行引擎,采用三层架构(TCB / Reactor / Governance),提供确定性执行、可审计链、时间旅行调试。

**架构原则**:机制与策略分离。核心层仅包含机制,应用层功能(HTTP API、SSE、Prometheus、认证、具体 I/O Handler)不在本仓。

### 🆕 新增

- 三层架构(机制层):evorule-tcb(纯计算内核,零依赖)/ evorule-reactor(反应器主循环,FactsLog + WAL + C FFI + 时间机器 + 调试控制)/ evorule-governance(治理层,SessionManager + Auditor + TimeMachine + IoDispatcher + RuleValidator)
- evorule-cli 命令行工具(validate / verify-chain / diff)
- 5 个 validate-*.ps1 + validate-all.ps1 校验脚本
- CI 流水线(Gitee Go + GitHub Actions)
- 形式化验证白皮书

### 🔒 安全

- SECURITY_AUDIT v0.1.0:P0 全修复,P1 4 项 HIGH 待公网部署前修复
- THREAT_MODEL.md:威胁建模
- DEPENDENCY_AUDIT v0.1.0:核心 crate 零已知漏洞

### 🔄 变更

- 协议统一为 AGPL-3.0-or-later
- 所有 .rs 文件加 SPDX header
- H5/H6 架构清理:HTTP API、SSE、Prometheus、认证、具体 I/O Handler 不在核心仓

### 🐛 修复

- Clippy 警告修复(全工作区零错误)
- 代码格式化(统一 Rust 官方格式)
- 差分测试修复(Reactor 运行时 vs Pure Function 一致性验证通过)
- Kani CI 修复(升级 Kani 版本,修正 nightly 兼容性)

### ⚠️ 已知问题

- Kani 部分 proof TIMEOUT(复杂 proof 因 CBMC 状态爆炸超时,由 proptest 兜底)
- 跨平台 release 未全面验证(Windows 开发验证通过,Linux/macOS CI 覆盖)
- 1.0 之前 API 仍可能变化
- 性能基准未建立(v0.2.0 引入 criterion 性能测试)

---

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evo-rule-lab/evorule>

---

**本变更日志采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
