# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260902-001 |
| **变更标题** | 审计/IO/会话四项强化：权限门 + WAL 显式拒绝 + 会话 CAS + diff 显式错误（UV-046 B6/B2/B4/B8b） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-02 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
四项均为机制层强化，不含任何业务语义：

- B6 权限门（io_subscriber.rs）：IoSubscriber 新增 gate 字段与
  with_permission_gate 装配方法，dispatch_and_respond 前经
  PermissionGate::check 做"入口仲裁"（拒绝时回写错误 IoResponse 并
  不分派）；谓词判定（谁能过门）留在应用层，机制层只提供门本身
- B2 WAL 显式拒绝（auditor.rs）：已弃用的 load_from_wal 对损坏行
  （非法 JSON / 条目校验失败）由静默跳过改为 InvalidData 错误，
  附 [EVO-AUDIT-WAL-CORRUPT] 标记与分级补救指引——审计完整性政策
  不允许静默丢证据
- B4 会话原子化（session.rs）：max_sessions 检查-占用改为 CAS
  循环（reserve_session_slot / release_session_slot），先原子占位
  再做昂贵操作，修复 TOCTOU 并发超额
- B8b diff 显式化（time_machine.rs）：diff 对 rewind 不可达版本由
  静默回退空 payload 改为返回 Result<PayloadDiff, TimeMachineError>
```

### 2.3 机制层判定标准检查

**✅ 机制层变更的特征**:
- [x] 提供通用基础设施能力（权限仲裁点 / 审计完整性 / 并发正确性 / 显式错误）
- [x] 不包含任何特定业务语义
- [x] 可被任何业务场景无差别复用

## 3. 变更分类

- **变更类型**: B - 机制扩展
- **影响模块**: evorule-governance/src/{io_subscriber,auditor,session,time_machine}.rs

## 4. 变更详情

### 3.1 变更理由

UV-046 report-002 四项发现：
- B6：`with_permission_gate` 文档承诺但未实现（文档-代码不符）
- B2：WAL 损坏行静默跳过 → 审计链不完整且无告警，违背审计完整性政策
- B4：会话计数 check-then-increment 非原子 → 并发创建可超 max_sessions
- B8b：diff 静默回退空 payload → 产生无告警的错误 diff 结果

### 3.2 变更范围

- io_subscriber.rs：gate 字段 + `with_permission_gate` + dispatch 前检查
  + 拒绝路径错误 IoResponse 回写 + 3 个单测
- auditor.rs：`load_from_wal` 三处静默路径改显式 InvalidData
  （[EVO-AUDIT-WAL-CORRUPT] + 文件/行号/原因/分级补救指引）
- session.rs：`reserve_session_slot`/`release_session_slot`（CAS），
  `create_session`/`create_session_from_parent_at_version` 先占位
- time_machine.rs：`TimeMachineError` + `diff` 返回 Result + 测试适配

### 3.3 破坏性分析

- B6：纯增量（不装配 gate 时行为与原来一致）
- B2：⚠️ 行为变化——损坏 WAL 从"跳过继续"变为"拒绝加载"；这是修复目的
  （静默丢证据不可接受），错误信息含补救指引
- B4：对外错误类型不变（LimitExceeded），仅时序正确性修复
- B8b：⚠️ API 签名变化——`diff` 返回 `Result`；调用方需适配
  （0.x 阶段以 MINOR 承载）

### 3.4 影响评估

- 全 workspace 测试须绿；evorule-server 消费侧同批适配（session_diff
  对 Err 映射 400 BAD_REQUEST，依赖升至 0.4.1）
- 权限门默认不装配，既有部署零行为变化

### 3.5 测试计划

- [x] B6：gate 拒绝 → 错误 IoResponse 回写且不分派；放行 → 正常分派；skip 谓词兼容
- [x] B2：损坏行/校验失败 → 显式错误含补救指引
- [x] B4：并发占位测试（CAS 语义）
- [x] B8b：不可达版本 → Err(TimeMachineError)
- [x] 全 workspace `cargo test` 回归

### 3.6 回滚方案

git revert 本提交即恢复原形态（B8b 调用方适配需同批回滚）。

## 5. 审查清单

### 层级审查
- [x] 变更层级声明为"机制层"
- [x] 判定理由充分
- [x] 代码中无策略层反模式

### 技术审查
- [x] 代码符合 GOVERNANCE_SPEC.md 规范
- [x] 单元测试通过

### 架构审查
- [x] 变更符合规则分类体系
- [x] 符合"机制不染指控制流"原则

---

## 6. 变更记录 CR-20260820-002: 添加变更治理门禁机制和策略层检测

> 归档说明：原 CR 整表置于顶层至 2026-09-02（CR-20260902-001 置顶），完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260820-002 |
| **变更标题** | 添加变更治理门禁机制和策略层检测 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-20 |
| **审查状态** | 已批准 |

build.rs 添加 CHANGE_REQUEST.md 验证逻辑和策略检测：通用的变更治理
基础设施（审查流程管理 + 代码质量保障），不含业务语义。回滚：删除
build.rs 中的验证代码。

## 7. 变更记录 CR-20260826-001: G-A1 审计锚点签名（v0.3.2 新增）

### 7.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260826-001 |
| **变更标题** | G-A1 审计锚点签名（signing 模块） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-26 |
| **审查状态** | 已批准 |

### 6.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 6.3 判定理由

```
ed25519 确定性签名 + 审计锚点校验是通用安全原语，不包含任何业务语义，
可被任何审计/合规场景无差别复用（真实性/防抵赖）。
```

### 6.4 变更详情

- 新增 `signing` 模块（`pub mod signing`），公开导出 `AuditSigner` / `SignError` / `verify_signature`
- ed25519（RFC 8032）确定性签名，无 RNG，兼容确定性执行纪律
- 私钥不落盘审计资产/规则库，由调用方注入 32 字节种子（未配置签名器时审计行为与旧版完全一致）
- `Cargo.toml` 新增依赖：`ed25519-dalek` / `getrandom`（仅密钥生成一次性运维用）

### 6.5 测试计划

- [x] `signing` 单元测试（确定性签名 / 验签 / 篡改载荷 / 篡改签名 / 错误公钥 / hex 往返）
- [x] 构建通过（CR 门禁 + 策略层检测 PASSED）

---

## 7. 变更记录 CR-20260830-001: build.rs 门禁状态机生命周期撇号判别修复

### 7.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260830-001 |
| **变更标题** | build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-30 |
| **审查状态** | 已批准 |

### 7.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 7.3 判定理由

```
本变更只修改 build.rs 门禁自身实现，不触及任何 src/ 执行语义：
- strip_test_mod/find_inline_lbrace/match_brace 状态机在撇号处新增
  char_lit_starts() 判别（字符字面量 vs 生命周期），新增 skip_lifetime() 跳过
- 修复前 'static 等生命周期撇号被误判为字符态开头，吞掉后续所有花括号，
  令 match_brace 永不闭合、tests 模块整体不被剥离、门禁对测试代码全量误报
- 五仓（tcb/reactor/governance/cli/server）同一份实现同步修复，
  每仓 build.rs 内含 3 个单元测试（探针 crate 验证）
- 不新增/删除任何扫描模式，语言规范能力不变
```

### 7.4 变更详情

- build.rs：新增 `char_lit_starts`/`skip_lifetime` 两个函数；`find_inline_lbrace`/
  `match_brace` 撇号入口处按判别结果分流；新增 `#[cfg(test)] mod tests`（3 个测试）
- 根 `GATE_REFERENCE.md`：登记修复说明与 evorule-server S 系列条目
- 门禁判定结果只会更精确（减少误报），不会放行任何原本被拦截的生产代码模式

### 7.5 测试计划

- [x] build.rs 内嵌 3 个单元测试（match_brace 生命周期/剥离存活/判别规则）
- [x] 探针 crate 以 lib.rs 方式加载真实 build.rs 运行（cargo test 不运行 build script 测试）
- [x] 核心四仓串行 cargo build 门禁 PASSED + evorule-server 全 workspace 编译通过

回滚：git revert 本提交即恢复旧状态机。

---

## 8. 变更记录 CR-20260830-002: IoSubscriber 跳过谓词（外部执行者应答权保障）

### 8.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260830-002 |
| **变更标题** | IoSubscriber 新增可选 skip 谓词（LLM 审计形态 IoRequest 不自动应答） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-30 |
| **审查状态** | 已批准 |

### 8.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 8.3 判定理由

```
跳过谓词是通用订阅者能力：命中谓词的 IoRequest 不由订阅者自动应答，
留给外部执行者处理。不含任何业务语义（LLM 审计形态的具体判定由应用层
evorule-server 挂载时注入，本仓只提供机制）。
```

### 8.4 变更详情

- `io_subscriber.rs`：新增 `SkipPredicate` 类型别名与 `with_skip` builder 方法；
  `handle_fact` 的 IoRequest 分支在 dispatch 之前先判谓词，命中则 trace 留痕并忽略
  （不回写任何 IoResponse）
- 默认 `skip = None`，历史行为完全不变（全部 IoRequest 照常自动应答）
- 解决的架构问题：server 侧 IoSubscriber 对 LLM 审计形态的 `call_external`
  （带 `messages`、无 `service_name`）会以 "missing service_name" 快速错误应答，
  抢占外部执行者（evo-agent AuditedLlm / console-cloud 浏览器审计桥）的应答权——
  外部执行者的 io_response 被反应器按 Unknown IoResponse 忽略，审计回路永远失败
- 谓词判定留在应用层（server），机制层不感知"LLM 审计"语义

### 8.5 测试计划

- [x] `test_skip_predicate_leaves_io_request_unanswered`：skip 命中 → 无任何 IoResponse 回写
- [x] `test_without_skip_error_response_is_written`：对照组，默认行为不变（错误应答回写）
- [x] `test_skip_predicate_does_not_hit_service_calls`：带 service_name 的调用不跳过
- [x] `cargo test -p evorule-governance --lib` 全绿；CR 门禁 + 策略层检测 PASSED

回滚：git revert 本提交即恢复无谓词状态。

---

## 9. 变更记录 CR-20260901-001: 审计增量化——audit_new 全量 clone 消除（UV-032 O(n²) 修复配套）

### 9.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260901-001 |
| **变更标题** | 审计增量化——audit_new 全量 clone 消除（配套 evorule-reactor Stable 瘦身） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-01 |
| **审查状态** | 已批准 |

### 9.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 9.3 判定理由

```
本变更只改 audit_new 的遍历方式（全量 clone → 增量迭代），审计语义
（去重游标 entries.len() / 条目结构 / 哈希链算法 / verify 与 replay）
逐条等价，不触及任何业务语义，可被任何审计场景无差别复用。
```

### 9.4 变更详情

- `auditor.rs` `audit_new()`：原每命令调用 `FactsLog::history()` 全量 clone
  全部历史事实（长驻会话 ~1500 命令时每命令近 GB 级内存复制，构成 O(n²)
  CPU 瓶颈主体）——改经 tier1 新增的 `FactsLog::for_each_fact_from`
  锁内零 clone 增量遍历尾部新事实
- 去重策略不变：仍以 `entries.len()` 为游标；哈希失败跳过与 entry_index
  计数语义与原 enumerate 对齐
- 配套 evorule-reactor CR-20260901-001（Fact::Stable 瘦身为 version）：
  hash.rs 测试构造、sse_integration_test 状态断言改经 FactsLog::snapshot()、
  end_to_end_audit_chain make_stable 适配

### 9.5 测试计划

- [x] `cargo test -p evorule-governance` 全绿（144 lib + 集成测试）
- [x] 增量遍历与全量 history() 语义一致性经既有 audit_new/verify 全量回归覆盖
- [x] 哈希快照重生成后 tier2 与 tier1 一致性测试通过

回滚：git revert 本提交即恢复全量 clone 形态。

---

> 注意：这是机制层变更，后续每次修改都需要更新此文件。
