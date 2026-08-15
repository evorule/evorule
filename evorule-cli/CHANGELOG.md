<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.2.4 - 2026-08-15

**版本同步 + 形式化验证 P0 完善（PATCH）** — 机制层 evaluate_domain 分层 Kani harness 落地与验证证据归档。CLI 无功能变更，依赖版本同步 bump。详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.2.4]` 段。

### 🔄 变更

- 版本同步 bump 至 0.2.4
- `Cargo.toml` 依赖版本对齐：`evorule-tcb` / `evorule-reactor` 由 `0.2.3` → `0.2.4`
- 发版计划：Gitee 发布 v0.2.4 时，crates.io 同步发布 v0.2.4（crates.io 当前停在 v0.2.1）

### 向后兼容

- ✅ 命令与输出格式不变
- ✅ fact log 格式不变

---

## v0.2.3 - 2026-08-10

**CLI 规则加载修复（PATCH）** — 修复 `load_rules` 将规则目录内的初始数据文件 `payload.json` 误当作规则加载的问题。详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.2.3]` 段。

### 🐛 修复

- **`load_rules` 排除保留数据文件 `payload.json`**（[io_util.rs](src/io_util.rs)）：
  - 规则目录内放置 `payload.json`（通常无 `type` 字段）此前会触发 `missing field: type`
  - 现按约定排除文件名恰好为 `payload.json`（大小写不敏感）的文件
  - 新增单测 `test_load_rules_ignores_payload_json`

### 🔄 变更

- 版本同步 bump 至 0.2.3
- `Cargo.toml` 依赖版本对齐：`evorule-tcb` / `evorule-reactor` 由 `0.2.2` → `0.2.3`
- 发版计划：Gitee 发布 v0.2.3 时，crates.io 同步发布 v0.2.3（crates.io 当前停在 v0.2.1）

### 向后兼容

- ✅ 合法规则文件的加载与确定性排序不变
- ✅ fact log 格式不变
- ✅ 命令与输出格式不变（仅修复误加载场景）

---

## v0.2.2 - 2026-08-10

**与 evorule-tcb / evorule-reactor / evorule-governance v0.2.2 同步** — 协议文档修正 + SDK 合规脚本方向反转（核心仓层面）。CLI 无源代码改动（仅版本同步 bump + 依赖版本号对齐 + README 版本 badge 更新）。详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.2.2]` 段。

### 🔄 变更

- 版本同步 bump 至 0.2.2
- `Cargo.toml` 依赖版本对齐：`evorule-tcb` / `evorule-reactor` 由 `0.2.1` → `0.2.2`
- README 版本 badge 更新（v0.2.1 → v0.2.2）
- README 下载链接回退至 v0.2.1（v0.2.2 未在 Gitee 发布二进制 release，下载指向最近一个有 release 的版本）

### 向后兼容

- ✅ CLI 行为不变
- ✅ fact log 格式不变

---

## v0.2.1 - 2026-08-05

**与 evorule-tcb / evorule-reactor / evorule-governance v0.2.1 同步** — v0.2.0 发布后 Kani 验证同步修正。CLI 无代码改动（仅版本同步 bump + verify.sh 重命名）。详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.2.1]` 段。

### 🔄 变更

- 版本同步 bump 至 0.2.1
- `verify-v0.1.0.sh` 重命名为 `verify.sh`（功能不变）
- README 版本 badge 更新

### 向后兼容

- ✅ CLI 行为不变
- ✅ fact log 格式不变

---

## v0.2.0 - 2026-08-04

**与 evorule-reactor v0.2.0 同步** — `executor.rs` 改用 `IoType::new(&io_type)` 构造。v0.2.0 起 `IoType` 重构为 `Arc<str>`(失去 `Copy`,5 个 `const` 改工厂函数,`parse` 标记 `#[deprecated]`)。CLI 行为不变:透传 io_type 不校验,无 handler 时发 `Fact::Error` 退出。

### 🔄 变更

- `executor.rs`:`IoType::parse(io_type)` → `IoType::new(&io_type)`(`parse` 已弃用)
- 版本同步 bump 至 0.2.0

### 向后兼容

- ✅ CLI 行为不变(透传 io_type,无 handler → Error)
- ✅ fact log 格式不变(io_type 字符串值不变)

详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.2.0]` 段 + [MIGRATION_v0.2.0.md](../MIGRATION_v0.2.0.md)。

---

## v0.1.1 - 2026-08-01

四 crate 工作区版本同步 bump(cli 无代码改动,仅版本同步至 0.1.1)。详见根 [CHANGELOG.md](../CHANGELOG.md) `[0.1.1]` 段。

---

## v0.1.0 (corrective rewrite) - 2026-07-26

### 重写动机

v0.1.0 初版(baseline 2026-07-25)存在多个根本性缺陷,导致 fact log 格式不兼容、
执行语义错误、哈希链承诺虚假。本次重写在保持 v0.1.0 版本号的前提下(全生态统一),
完全重写 CLI 以修复所有已知问题。

### Breaking Changes(fact log 格式)

fact log 从自定义 JSON 格式改为 **evorule-reactor WAL JSONL 格式**,与 evorule-governance
审计链互通。旧 fact log 不兼容新 `replay`/`diff`/`verify-chain` 命令。

**旧格式**(v0.1.0 baseline):

```json
{"step":1,"type":"state_transition","new_payload":{"x":10}}
{"total_steps":2,"type":"final","final_payload":{"x":10}}
```

**新格式**(v0.1.0 rewrite):

```json
{"id":1,"instruction":{"type":"noop"},"type":"Command"}
{"cause":1,"id":2,"new_payload":{"x":42},"new_queue":[],"type":"StateTransition"}
{"final_snapshot":{"x":42},"id":3,"type":"Stable"}
```

### P0 修复(执行正确性)

- **FIFO 队列**:`VecDeque::pop_front()` 替代 `Vec::pop()`(LIFO bug)
  - tier0 `exec_push` 用 `new_queue.append(queue)` 把新指令前置(插队语义)
  - 必须从前端取才能保证 push 的指令先执行
- **确定性加载**:`load_rules` 按 `file_name()` 字典序排序后加载
  - 消除 `fs::read_dir` 顺序差异(Windows NTFS 字典序 vs Linux ext4 hash 序)
  - 保证同目录规则在不同平台执行结果一致
- **max_steps 先检后 pop**:对齐 evorule-reactor BUG-3 修复,超限发 `Fact::Error` + break
- **I/O 两阶段架构**:`pending_io: HashMap<FactId, JsonValue>` 缓存 orig 指令
  - 0.1.0 无 handler 时发 `Fact::Error` 退出,但架构正确
  - 后续加 handler 时只需在 IoRequest 分支注入 IoResponse + push_front(orig) 即可

### P1 修复(哈希链 + 结构校验)

- **新增 `verify-chain` 子命令**:三层验证 fact log 完整性
  1. blake3 哈希链(与 evorule-governance 字节级一致)
  2. FactId 单调递增校验
  3. cause 引用有效性校验(StateTransition.cause / IoRequest.cause 必须指向已存在 FactId)
- **hash.rs 复制自 evorule-governance**:4 个公开函数算法体字节级一致
  - `include_str!` 交叉验证测试强制双边同步
  - `HashError` 简化(去 Backtrace),哈希值不变
- **篡改检测**:修改 FactId 破坏单调性 → verify-chain 报错退出 1

### P2 修复(diff 语义)

- **diff 按 FactId 数组下标对齐**:替代原 `HashSet::difference`(丢失重复行且无序)
  - `[~]` 两边都有但内容不同
  - `[-]` 只在 A
  - `[+]` 只在 B
  - `(identical)` 完全相同

### 模块化架构

从单文件 `main.rs` 拆分为 8 个模块 + 5 个子命令模块:

```
src/
├── main.rs              # 入口:tracing + 子命令分发
├── cli.rs               # clap derive 参数定义
├── error.rs             # CliError 枚举 + 退出码映射(0/1/2)
├── executor.rs          # 同步反应器循环(FIFO + max_steps + I/O 两阶段)
├── fact_log.rs          # JSONL 读写(tier1 WAL 格式)
├── hash.rs              # blake3 哈希链(复制自 evorule-governance)
├── io_util.rs           # 规则加载(确定性排序)+ payload 解析
├── output.rs            # human-readable 格式化 + diff 前缀
└── commands/
    ├── mod.rs
    ├── validate.rs      # core_eval 元指令白名单校验
    ├── run.rs           # 加载→执行→输出 fact log
    ├── replay.rs        # 读 fact log → pretty-print
    ├── diff.rs          # 按 FactId 数组下标对齐比对
    └── verify_chain.rs  # 哈希链 + 结构不变量
```

### build.rs 增强

- **递归扫描 `src/**/\*.rs`**:从单文件 `src/main.rs` 扩展到递归扫描,确保模块化后门控有效
- **`strip_test_mod` 剥离测试模块**:通过花括号计数(感知字符串/字符/注释),使测试内的 G8/F11 模式不触发误报
- **零豁免原则**:删除 `VALID_TRANSFORM_TYPES` 豁免(validate.rs 用 core_eval 元指令白名单,不含 G8 禁止词)

### validate 子命令调整

- **未知 type 从 WARN 改为 ERROR**:原设计 `[WARN]` + exit 0 过于宽松,改为 `[ERROR]` + exit 1
- **退出码调整**:empty/nonexistent 目录从 exit 1 改为 exit 2(区分"环境错误"与"验证错误")
- **白名单来源**:从手写白名单改为 core_eval 元指令白名单(branch/set/push/io_request/noop/increment/decrement)

### 依赖范围

- `evorule-tcb`(纯函数 `execute_transition`)—— 核心 TCB
- `evorule-reactor`(`Fact`/`FactId`/`IoType`/`wal`/`serde_to_tcb`)—— Fact 序列化
- `blake3` —— 哈希链(复制自 evorule-governance,避免拉入 axum/sqlx/reqwest 破坏 musl)
- 不依赖 `evorule-governance`(避免破坏 musl 静态链接)
- 不创建 tokio runtime(`execute_transition` 是同步纯函数)

### e2e 测试

从 19 个测试扩展到 **28 个 TAP 端到端测试**,新增覆盖:

- verify-chain(valid + tampered)
- FIFO 队列顺序(push [step1,step2] → step1 先执行)
- 确定性加载(3 文件按文件名排序,所有字段设置)
- max-steps 限制(--max-steps 0 → Error fact)
- hospital example IoRequest + Error(无 handler)
- verify-chain on hospital log(复杂 fact 序列)
- JSONL 完整性(每行含 type + id 字段)

### Fixture 修复

- `valid/rule.json`:`attr` 从 `"__exec__.payload.x"`(路径引用 bug)改为 `"x"`(字面字段名)
- `examples/hospital/rules/*.json`:io_type 从业务语义(`audit_log`/`check_doctor_credentials`/...)改为 `call_service` + `service_name`(mechanism-policy separation)
- `examples/law-firm/rules/*.json`:同上 + `set` 指令 `attr` 路径引用修复
- 新增 `echo/rule.json`:复制 payload.input 到 result(用于 diff 测试)
- 新增 `fifo/rule.json`:push [step1,step2] + 分支匹配,验证 FIFO 队列顺序
- 新增 `multi/01-set-a.json` + `02-set-b.json` + `03-set-c.json`:3 文件确定性加载测试

### 能力

- ✅ **零网络** —— 不调用任何外部服务
- ✅ **零遥测** —— tracing 只写 stderr
- ✅ **零 AI 决策** —— 不调用 LLM,纯确定性执行
- ✅ **零系统依赖** —— musl 静态链接,1.8 MB 单文件
- ✅ **多架构** —— `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl`
- ✅ **可重现构建** —— `build-musl.sh --repro` 两次构建 SHA256 一致
- ✅ **G8 门控** —— 递归扫描 + strip_test_mod,零豁免
- ✅ **blake3 哈希链** —— 与 evorule-governance 交叉验证
- ✅ **结构不变量校验** —— FactId 单调 + cause 引用
- ✅ **FIFO 队列** —— pop_front,修复原 LIFO bug
- ✅ **确定性加载** —— 文件名排序,跨平台一致
- ✅ **e2e 测试** —— 28/28 PASS

### 5 个子命令

| 子命令                                                                                  | 用途                                      |
| --------------------------------------------------------------------------------------- | ----------------------------------------- |
| `evorule validate <rules-dir>`                                                          | 校验 JSON 规则(core_eval 元指令白名单)    |
| `evorule run <rules-dir> [--payload X \| --payload-file X] [-o output] [--max-steps N]` | 执行规则 + 输出 fact log(tier1 WAL JSONL) |
| `evorule replay <fact-log>`                                                             | 重放 fact log(pretty-print)               |
| `evorule diff <a.log> <b.log>`                                                          | 对比两个 fact log(按 FactId 对齐)         |
| `evorule verify-chain <fact-log>`                                                       | 验证 fact log 完整性(哈希链 + 结构不变量) |

### 已知限制(0.1.0)

- ❌ 无 I/O handler(`io_request` 会产生 IoRequest fact + Error fact,不实际执行 I/O)
- ❌ 无 HTTP API(那是 `evorule-server` 独立仓的事,evorule 核心仓纯 lib + 本地 CLI)
- ❌ 无配置文件(后续加 `.evorule.toml`)
- ❌ 无 hot-reload(后续加)

### 配套示例

- [`examples/hospital/`](examples/hospital/) —— 医院 HIPAA / 等保 2.0 合规规则
- [`examples/law-firm/`](examples/law-firm/) —— 律所客户保密 / GDPR 合规规则

详见 [`README.md`](README.md) + [`CLI_SPEC.md`](CLI_SPEC.md)。

---

## v0.1.0 (baseline) - 2026-07-25 (EvoRule 公开 baseline)

随 [`evorule` v0.1.0](https://gitee.com/evo-rule-lab/evorule) 同步发布,作为
evorule workspace 内的子 crate。

### 已知缺陷(已在 v0.1.0 corrective rewrite 中修复)

- ❌ LIFO 队列 bug(`Vec::pop()` 而非 `VecDeque::pop_front()`)
- ❌ 规则加载顺序不确定(依赖 `fs::read_dir` 返回顺序)
- ❌ fact log 格式与 evorule-reactor WAL 不互通(snake_case 自定义格式)
- ❌ diff 使用 `HashSet::difference`,丢失重复行且无序
- ❌ 无哈希链验证(README 营销对比表承诺"哈希链"但未实现)
- ❌ build.rs 只扫描 `src/main.rs`,模块化后门控失效
- ❌ validate 未知 type 为 WARN(过于宽松)
