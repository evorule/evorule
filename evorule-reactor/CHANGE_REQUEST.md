# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260831-001 |
| **变更标题** | 存储层 trait 抽象：FactWalStore 后端契约 + MemoryWalStore 内存后端（UV-026） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-31 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
本变更为 FactsLog 的 WAL 持久化层提供可替换后端契约，不触及任何业务语义：
- wal.rs 新增 FactWalStore trait（单方法 append_record_with_hash，write-ahead
  语义：内存更新前调用，Ok 即承诺记录不丢失）与 MemoryWalStore 内存后端
  （Arc<Mutex<Vec<WalRecord>>>，Clone 为共享句柄语义，供无文件系统/嵌入式/
  测试场景与事后检视）；WalWriter 经纯委托实现 trait（逐字节行为不变）
- facts_log.rs：FactsLogInner.wal 由 Option<WalWriter> 改为
  Option<Box<dyn FactWalStore>>（事实：FactsLog 对 WAL 的写调用本就仅此一处）；
  新增构造器 with_wal_store；new/with_wal*/recover*/compact/reset 公开 API
  与文件后端行为零改动
- 哈希链计算、版本推进、恢复重放逻辑全部不动；消费方（governance
  SharedFactsLog / server SessionManager）公开 API 零改动
```

### 2.3 机制层判定标准检查

**✅ 机制层变更的特征**:
- [x] 提供通用基础设施能力（存储后端可替换契约）
- [x] 不包含任何特定业务语义
- [x] 可被任何业务场景无差别复用（第三方可接 SQLite/远程后端）

## 3. 变更分类

- **变更类型**: B - 机制扩展
- **影响模块**: evorule-reactor/src/wal.rs、src/facts_log.rs、src/lib.rs、CHANGE_REQUEST.md

## 4. 变更详情

### 3.1 变更理由

"一切皆 plugin" 架构原则第四章第3项（UV-026）：WAL/事实存储后端可替换，
当前实现与引擎耦合。盘点确认 FactsLog 内存层与文件层本已分离（`new()` 纯内存、
`recover*()` 文件恢复），缺的只是写侧可替换点——抽为 trait 即得最小真边界。

### 3.2 变更范围

- wal.rs：`FactWalStore` trait、`impl FactWalStore for WalWriter`（纯委托）、
  `MemoryWalStore`（含 len/is_empty/records/into_records）
- facts_log.rs：`wal` 字段类型改为 trait 对象；`recover_with_options`/
  `with_wal_options` 挂载点包 `Box::new`；新增 `with_wal_store`
- lib.rs：导出 `FactWalStore`、`MemoryWalStore`
- 新增 3 个单测：内存后端往返哈希字段保真 / 内存后端与纯内存模式哈希链一致 /
  文件后端经 trait 分发回归锁

### 3.3 破坏性分析

无对外契约变化。`FactsLog` 公开 API（new/with_wal*/recover*/compact/reset/
append/read_from 等）签名与行为不变；默认文件后端经 trait 纯委托，WAL 文件
格式与字节行为不变；`--no-default-features` 构建不受影响（trait 同在
persistence 门控内）。

### 3.4 影响评估

- evorule-reactor：137 过（no-default）+ 181 过（all-features，含 3 新测试）；
  变更治理门禁 PASSED
- evorule-governance：144 过（消费方零改动回归）
- evorule-server：192 过（path 依赖直接吸收，零改动回归）

### 3.5 测试计划

- [x] MemoryWalStore 往返：哈希三字段与 version_before 逐项保真
- [x] FactsLog::with_wal_store + MemoryWalStore 与纯内存模式 last_hash/version/
  历史长度一致（哈希链语义不变）；后端记录与内存 history 按 version_before 同相
- [x] 文件后端经 trait 分发：recover 后 last_hash 与追加时一致（回归锁）
- [x] 三仓全量库测试绿（reactor/governance/server）

### 3.6 回滚方案

git revert 本提交即恢复 `Option<WalWriter>` 直挂形态；MemoryWalStore 与
with_wal_store 为纯新增，revert 无残留影响。

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
