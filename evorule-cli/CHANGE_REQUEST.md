# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260902-001 |
| **变更标题** | run 命令 Error fact 退出码 3 + validate 白名单 SSOT 化（UV-046 C1/C3 + C2 消费侧） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-02 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
- C1/C3：`evorule run` 执行含 Error fact 时原返回退出码 0（仅 tracing
  告警）——CI/自动化管道以退出码判定成败，Error fact 静默成功让
  "确定性执行"承诺在自动化场景失效。新增 CliError::ExecutionHadErrors
  （退出码 3）；fact log 仍正常写出供审计定位失败原因
- C2：validate 命令的元指令白名单由本地硬编码副本改引
  evorule_tcb::META_INSTRUCTION_TYPES（SSOT），消除与 tcb dispatch
  的漂移风险（tcb 侧 CR-20260902-001 提供常量与漂移防线单测）
```

### 2.3 机制层判定标准检查

**✅ 机制层变更的特征**:
- [x] 提供通用基础设施能力（自动化管道可感知的失败信号 / 类型清单单一事实源）
- [x] 不包含任何特定业务语义
- [x] 可被任何业务场景无差别复用

## 3. 变更分类

- **变更类型**: B - 机制扩展（含 ⚠️ 行为变化：run 退出码语义）
- **影响模块**: evorule-cli/src/{error.rs,commands/run.rs,commands/validate.rs}

## 4. 变更详情

### 3.1 变更理由

UV-046 report-002：
- C1/C3：CI 场景 `evorule run` 对规则执行失败返回 0，自动化管道无法感知
- C2：validate 白名单与 tcb dispatch 双份维护，存在漂移风险

### 3.2 变更范围

- error.rs：新增 `ExecutionHadErrors { count }` 变体（退出码 3），
  `exit_code()` 映射 + 文档/单测跟随
- run.rs：执行完成统计 Error fact 数量，>0 返回 `ExecutionHadErrors`
- validate.rs：删除本地 `VALID_TRANSFORM_TYPES` 硬编码，改引
  `evorule_tcb::META_INSTRUCTION_TYPES`（含测试）

### 3.3 破坏性分析

⚠️ 行为变化（0.x MINOR 承载）：`evorule run` 含 Error fact 时退出码
0 → 3。这是修复目的；依赖"总是返回 0"的脚本需改按退出码分支。
fact log 输出行为不变。

### 3.4 影响评估

- 全 workspace 测试须绿；`exit_code` 文档测试覆盖新映射
- 退出码表：0 成功 / 1 通用错误 / 2 规则加载错误 / 3 执行含 Error fact

### 3.5 测试计划

- [x] `test_exit_code_mapping`：ExecutionHadErrors → 3
- [x] validate 白名单断言测试改引常量后全绿
- [x] 全 workspace `cargo test` 回归

### 3.6 回滚方案

git revert 本提交即恢复退出码 0 形态（与 reactor 侧 Error fact 入链
变更同批实施，回滚需同批处理——否则 run 仍会收到 Error fact 但静默成功）。

## 5. 审查清单

### 层级审查
- [x] 变更层级声明为"机制层"
- [x] 判定理由充分
- [x] 代码中无策略层反模式

### 技术审查
- [x] CLI 功能测试通过
- [x] 代码质量检查通过

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
基础设施，不含业务语义。回滚：删除 build.rs 中的验证代码。

## 7. 变更记录 CR-20260826-001: G-A1 审计锚点签名（v0.3.2 新增）

### 7.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260826-001 |
| **变更标题** | G-A1 审计锚点签名（anchor-keygen / verify-anchors） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-26 |
| **审查状态** | 已批准 |

### 6.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 6.3 判定理由

```
ed25519 确定性签名 + 审计锚点链式校验是通用安全原语，不包含任何业务语义，
可被任何审计/合规场景无差别复用（真实性/防抵赖）。
```

### 6.4 变更详情

- 新增 `signing` 模块（`AuditSigner` / `verify_signature`，复制自 evorule-governance 最小实现，遵循 CLI 不引入 governance 的架构约束）
- 新增 `anchor-keygen` 子命令：生成 G-A1 审计锚点 ed25519 密钥对（一次性运维）
- 新增 `verify-anchors` 子命令：离线校验审计锚点链式链接 + 签名真实性
- `Cargo.toml` 新增依赖：`ed25519-dalek` / `getrandom`（仅密钥生成一次性运维用，不进入确定性执行路径）
- 私钥不落盘审计资产/规则库，由调用方注入 32 字节种子

### 6.5 测试计划

- [x] `signing` 单元测试（确定性签名 / 验签 / 篡改检测 / hex 往返 / 密钥生成）
- [x] `verify_anchors` 单元测试（链式链接 / 篡改 / 断链 / 错误公钥）
- [x] `anchor-keygen` 冒烟测试通过
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

## 8. 变更记录 CR-20260901-001: executor 最终 payload 直接返回 + Stable 新结构适配（UV-032 O(n²) 修复配套）

### 8.1 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260901-001 |
| **变更标题** | executor 最终 payload 直接返回 + Stable 新结构适配 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-01 |
| **审查状态** | 已批准 |

### 8.2 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 8.3 判定理由

```
本变更适配 tier1 Fact::Stable 瘦身（快照字段移除，CR-20260901-001），
CLI 单次运行结果的取值路径从事实链改为执行器返回值——机制层取值路径
调整，不含任何业务语义。
```

### 8.4 变更详情

- `executor.rs`：`execute()` 返回值 `Vec<Fact>` → `(Vec<Fact>, JsonValue)`，
  最终 payload 由执行器持有并直接返回（不经事实链）；内部新增 version
  计数（每条 StateTransition +1，对齐 reactor 语义），Stable 发射
  `version` 字段
- `run.rs` / `examples/programmatic_run.rs`：解构新返回值
- `fact_log.rs` / `output.rs` / `diff.rs` / `verify_chain.rs` / `hash.rs`：
  测试构造适配（Stable 用 version）；`fact_to_human` Stable 行显示
  `version=N`；fact_log 保留旧格式字符串样例兼作向后兼容解析测试
- `tests/cli_test.rs`：FIFO 断言从"末行 Stable 快照"改为"最后一条
  StateTransition 的 new_payload"

### 8.5 测试计划

- [x] `cargo test -p evorule-cli` 全绿（61 lib + 20 集成）
- [x] 旧格式（≤0.3.x 含 final_snapshot）fact log 解析兼容性测试保留通过
- [x] FIFO / 确定性加载 / verify-chain / diff 端到端命令回归通过

回滚：git revert 本提交即恢复快照取值路径。

---

> 注意：这是机制层变更，后续每次修改都需要更新此文件。
