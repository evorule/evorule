# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260901-001 |
| **变更标题** | 单会话长跑 O(n²) 缺陷修复：Fact::Stable 瘦身 + WAL 旧格式容错 + FactsLog 增量迭代接口（UV-032） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-01 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
本变更修复事实模型在长驻会话场景下的结构性膨胀与审计遍历的全量 clone，
不触及任何业务语义：
- Fact::Stable 由 final_snapshot（全量 payload 快照）瘦身为 version: u64——
  recover 对 Stable 仅更新 last_stable_version、从不读取快照内容，快照为
  纯冗余；状态本体由最近一条 StateTransition.new_payload 确定，消费方经
  snapshot API 获取，信息零丢失
- wal.rs 序列化对齐新结构；反序列化对旧格式（≤0.3.x 含 final_snapshot）
  容错：忽略快照内容，version 缺失以 version_before 兜底
- facts_log.rs 新增 for_each_fact_from(start, f)：锁内零 clone 增量遍历
  尾部事实，供 tier2 Auditor 增量审计，消除每命令全量 clone 的 O(n²) CPU 瓶颈
- reactor.rs 4 个 Stable 发射点全部改传 state.version，哈希链算法与
  WAL 写入路径语义不变
```

### 2.3 机制层判定标准检查

**✅ 机制层变更的特征**:
- [x] 提供通用基础设施能力（事实模型瘦身 + 增量迭代接口）
- [x] 不包含任何特定业务语义
- [x] 可被任何业务场景无差别复用（长驻/单次运行均受益）

## 3. 变更分类

- **变更类型**: B - 机制扩展（含 ⚠️ 破坏性：审计链哈希输入与 WAL 磁盘格式变更，0.x MINOR 承载）
- **影响模块**: evorule-reactor/src/{fact,reactor,hash,wal,facts_log,channel,lib}.rs、
  tests/、examples/、verification/kani_proofs.rs、README.md；
  消费方 evorule-governance / evorule-cli（同批适配）

## 4. 变更详情

### 3.1 变更理由

UV-032 实战检验发现单会话长跑性能线性恶化（~1500 命令 → 100MB WAL、
2.52s/命令）：Stable 全量快照每命令 O(n) 入链累计 O(n²)；恢复路径从不
读取该快照，属纯冗余。审计器全量 clone 为另一 O(n²) 源（governance 侧
配套修复）。完整方案见知识库《24-UV032-单会话O2缺陷-修复方案设计.md》。

### 3.2 变更范围

- fact.rs：`Stable { id, final_snapshot }` → `Stable { id, version: u64 }`，
  to_json/type_name 等跟随
- reactor.rs：4 个 Stable 发射点（中断恢复/正常稳定/MaxRounds/队列上限）
  改传 `state.version`
- hash.rs：`fact_to_stable_json` 序列化对齐（审计链哈希输入变更）
- wal.rs：fact_to_json 写 version；fact_from_json 旧格式容错
  （final_snapshot 忽略 + version_before 兜底，由 read_wal_file_with_hash 补齐）
- facts_log.rs：新增 `for_each_fact_from`（零 clone 锁内增量迭代）
- channel.rs / lib.rs / README.md：测试构造与文档注释同步
- tests/{integration,complex_rule}_test.rs：状态断言改经 `FactsLog::snapshot()`
- examples/generate_hashed_wal.rs、verification/kani_proofs.rs：构造适配

### 3.3 破坏性分析

⚠️ 两处破坏性（0.x 阶段以 MINOR 承载，CHANGELOG 声明）：
1. 审计链哈希输入变化：Stable 哈希由含全量快照变为含 version → 旧 WAL 的
   chain_hash 在新代码 verify 下不匹配（恢复不受影响：recover 与链校验解耦）
2. WAL 磁盘格式：新代码可读旧格式（容错设计）；旧代码不可读新格式——
   升级单向，Release Notes 声明

### 3.4 影响评估

- 全 workspace `cargo test` 全绿（reactor 180 / governance 144+ / cli 61+20）
- lib clippy 无新增告警；无 panic 路径新增（for_each_fact_from 用 get 切片）
- 长会话 WAL 体积仍随命令数线性增长（StateTransition payload 为恢复机制
  本体，本轮不动，边界已在方案文档声明）

### 3.5 测试计划

- [x] 全部既有测试适配后通过（含 recover 往返、WAL 轮换、哈希快照重生成）
- [x] 旧格式 WAL 解析兼容性测试（fact_log.rs 保留旧格式字符串样例）
- [x] 增量迭代接口与 history() 语义一致性经 governance audit_new 全量回归覆盖
- [ ] bench_long_session 10000 命令复测（验收门禁 4.3，另行执行）

### 3.6 回滚方案

分支 fix/uv032-o2-stable-slim 独立实施，基线 tag pre-o2-fix-20260901；
`git revert` 提交 7da4045 即整体回滚。新格式 WAL 不能被旧代码恢复，
回滚需连同沙箱验证数据一并丢弃（验证均在 TEMP 沙箱，无生产数据影响）。

## 5. 审查清单

### 层级审查
- [x] 变更层级声明为"机制层"
- [x] 判定理由充分
- [x] 代码中无策略层反模式

### 技术审查
- [x] 代码符合 REACTOR_SPEC.md 规范
- [x] 单元测试通过

### 架构审查
- [x] 变更符合规则分类体系
- [x] 符合"机制不染指控制流"原则

---

## 附 · 历史变更归档

### CR-20260831-001（已批准）：存储层 trait 抽象：FactWalStore 后端契约 + MemoryWalStore 内存后端（UV-026）

> 归档说明：原 CR 整表收录于 2026-09-01（CR-20260901-001 置顶），完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260831-001 |
| **变更标题** | 存储层 trait 抽象：FactWalStore 后端契约 + MemoryWalStore 内存后端（UV-026） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-31 |
| **审查状态** | 已批准 |

机制层变更：FactsLog 的 WAL 持久化层提供可替换后端契约（FactWalStore trait +
MemoryWalStore 内存后端），哈希链计算、版本推进、恢复重放逻辑全部不动，
消费方公开 API 零改动。回滚：git revert。

---

### CR-20260830-001（已批准）：build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除）

> 归档说明：原 CR 整表收录于 2026-08-31，完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260830-001 |
| **变更标题** | build.rs 门禁状态机生命周期撇号判别修复 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-30 |
| **审查状态** | 已批准 |

机制层变更：strip_test_mod/find_inline_lbrace/match_brace 状态机在撇号处新增
char_lit_starts() 判别（字符字面量 vs 生命周期），修复 'static 被误判为字符态
致 tests 模块不剥离、门禁全量误报；五仓同一份实现同步修复。回滚：git revert。

---

### CR-20260820-002（已批准）：添加变更治理门禁机制和策略层检测

> 归档说明：原 CR 整表收录于 2026-08-30，完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260820-002 |
| **变更标题** | 添加变更治理门禁机制和策略层检测 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-20 |
| **审查状态** | 已批准 |

机制层变更：为 build.rs 添加 CHANGE_REQUEST.md 验证逻辑和策略层反模式检测，提供通用的变更治理基础设施；可被任何机制层代码复用，不含业务语义。回滚：删除 build.rs 中的验证代码即可。

---

> 注意：这是机制层变更，后续每次修改都需要更新此文件。
