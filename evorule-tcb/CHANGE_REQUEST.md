# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260913-004 |
| **变更标题** | B 档 23 个超时 proof 四梯队攻坚：CR-003 载体修订（回退 impl 级 cfg 覆写，改 proof 层 stub）+ harness 结构自检 + 模型偏差登记簿治理 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-13 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
CR-001（KaniMap）+ CR-002（递归有界化）+ CR-003（eq/clone 模型化）
消解三层根因后，B 档 proof 攻坚进入 proof 结构层面：单点修复模式
（修生产代码让原 proof 收敛）已被 round 1-12 探针证伪——三层任留
一层即 150s+ 超时，乘积结构必须拆解。本变更确立四梯队结构：
- Tier 0 可行性门（T0-1~T0-7 短实验：solver/stubbing/contracts
  可用性、算术与 clone 数据流审计、unwind 敏感度、构造层复审）
- Tier 1 零侵入优化（unwind 精确化、harness 构造迁移 owned 变体）
- Tier 2 Kani 机制层（proof 层 stub、contracts 组合验证）
- Tier 3 属性分流（深度类→TLA+、溢出类→类型系统门禁、
  已具体化输入→集成测试）
- Tier 4 治理（模型偏差登记簿 + 爆炸半径映射 + 补偿合规检查）
配套纪律：B 档 harness 结构自检断言硬前置；总预算 ≤60 次验证运行
× ≤600s；每阶段 kill criteria，任意检查点可降级收尾。
载体决策依据 ADR-0002（模型弱化载体：impl 级 cfg → proof 层 stub）。
```

### 3.1 变更理由

B 档 23 个 proof 覆盖 P0-1/2/4/5/7/8 六个 P0 属性，实测 600s/3600s
全超时（STATUS.md 附录 B）；三层根因修复后仍超时证明乘积爆炸结构
需 proof 结构层拆解，而非继续单点修生产代码。

### 3.2 变更范围

- **CR-20260913-003 载体修订**（本文件该 CR 修订记录节）：value.rs
  两处 impl 级 cfg(kani) 覆写（Clone/PartialEq）回退，模型化改由
  proof 层 `#[kani::stub(...)]` 承载
- tests/kani/kani_proofs.rs：B 档 harness 加结构自检断言（W3-1，
  Phase 1 前落地）；探针代码（rounds 2-12）迁出生产 proof 文件
  （Batch 1 完成迁出与 harness 对齐）
- KaniMap/ObjectMap 配套适配（CR-001 存储后端的调用面跟随，编译
  必需）：executor.rs（含 mem::take 全值移出修复——cfg(kani) no-op
  Drop 下部分移出被拒）、lib.rs（ObjectMap 顶层 re-export）、
  path.rs / domain.rs / determinism_proptest.rs（测试面 ObjectMap
  采纳——kani CI 以 `--tests` 编译 lib test 与集成测试目标，HEAD 版
  BTreeMap 构造在 kani 下 E0308）、tests/kani/model.rs、reactor 侧
  仅测试/ffi 便利采纳（非 kani 编译面，随在途改动另行入库）
- kani.yml：stub 化 proof 批次与 `-Z stubbing` 参数（Phase 2）
- verification/STATUS.md：属性表新增"模型偏差"列（Phase 1 落地）
- cfg(kani) 偏差登记簿（K 系）：新增"爆炸半径"列与补偿合规规则
  （爆炸半径全调用点映射 / 证据-实现绑定 / 补偿不得引用受该模型
  影响的 proof——S12/S13 机器检查草案）

### 3.3 破坏性分析

无。生产构建（cfg(not(kani))）零改动；A 档 14 个 proof 不经 eq/clone
容器路径，回退后须重跑确认（Batch 1 门禁：14/14 绿才提交）。

### 3.4 影响评估

- A 档证据 SHA 绑定随 value.rs 回退重核：Batch 1 在 WSL 重跑 A 档
  14 个，新 SHA 证据补档（P0-3/P0-6 证据列同批更新）
- reactor 4 个闸门 proof：stub 载体下零影响（爆炸半径限于声明
  stub 的 harness）；cfg 回退场景下需重新论证（G1 强制）
- 登记簿 K-4/K-5 补偿：stub 载体下挂 A 档
  `verify_partial_eq_never_panics`（真实现侧证明）成立；
  cfg 回退场景下降级为测试级补偿并显式标注

### 3.5 测试计划

- [x] Batch 1：value.rs 回退 + cargo test 全绿 + A 档 14 个 WSL
      重跑全绿（新 SHA 证据）+ check_doc_safety/check_status_sync 过
      （2026-09-13 实测通过；新 SHA 证据随本批归档提交落盘）
- [x] Phase 0：T0-1~T0-7 可行性矩阵定稿（每项 go/no-go 有实测/审计依据；
      2026-09-13 完成，结论摘要见 §3.7 追记）
- [ ] Phase 1：P8 系 9 个 proof × ≤3 配置（基线/精确 unwind/owned
      迁移），单 proof 中位数 ≤300s 为过关线
- [ ] Phase 2：stub 试点 2 个（eq 族 + 元指令族）≤600s；批量 ≤20 次
- [ ] Phase 3+4：分流落地 + 全文档对齐 + release-gate 演练
- [ ] 全程：机时记账累计 ≤60 次，触顶即降级收尾；各阶段停止
      准则——Phase 1 单 proof 三配置中位数 >300s 移交后续阶段路由，
      Phase 2 试点 >600s 该属性族移交 TLA+/降级，TLA+ 试跑 >3600s
      维持 N_MAX=2 如实标注

### 3.6 回滚方案

分批提交（Batch 1-5），每批原子对应代码+证据+状态+披露；任一批次
失败 revert 该批即可，前批成果（如 A 档新证据）不受影响。整体降级
路径：预算触顶或 Phase kill → 按收尾选项执行（B 档如实标 ❌，
根因档案与偏差登记簿定稿归档）。

### 3.7 Phase 0（T0-1~T0-7）可行性验证结论追记（2026-09-13 实测定稿）

| 项 | 结论 | 依据 |
|---|---|---|
| T0-1 求解器选项 | `--solver` 选项存在（bitwuzla/cadical/cvc5/kissat/minisat/z3/bin=，**默认 CaDiCaL**）；kissat 对 eq proof 单点试跑 600s 超时无改善 → **Tier 1.2 裁撤** | E1 实测（600s，机时记账） |
| T0-2 stubbing | `-Z stubbing` 最小 stub proof 实跑 PASS → **Tier 2.1 stub 默认路径确认**（cfg 回退预案解除待命） | E2 实测 |
| T0-3 contracts | `requires`/`ensures`/`proof_for_contract` 实跑 PASS；`stub_verified` 编译期报错（`Failed to find contract closure`）→ **组合验证改两步独立**（stub 自带 contract + `proof_for_contract`），P19-P21 主路径维持 | E3 实测 |
| T0-4 算术审计 | set/add/sub 路径全 `checked_add`/`checked_sub` + `IntegerOverflow` 传播，无裸算术（仅 usize 良性位点）→ **P13 可走 checked-ops 门禁卸载** | 代码审计（executor.rs） |
| T0-5 clone 内容依赖 | evaluate_eq 子树内 eq 结果与 clone 内容仅流入 never-panic 的 `PartialEq` 与分支控制流 → **clone stub 固定值分支成立（限该子树）** | 代码审计（domain.rs/executor.rs） |
| T0-6 unwind 敏感度 | `"payload.x"`→`"p.x"` + `unwind(24)`→`unwind(6)` 单点对比仍 600s 超时 → **Tier 1.1 单独无效**；harness 完全具体输入仍爆炸，证实结构层建模成本（String/Cow/Vec/分配器）主导——三层根因模型外残余因素（入根因档案） | E4 实测（600s）+ 代码审计（model.rs 无符号输入） |
| T0-7 构造层复审 | F1 中毒路径（impl 级 cfg Clone 覆写经 `object_from_pairs` 清空嵌套复合值）已随 CR-20260913-003 修订消除；现存 `#[cfg(kani)]` 属性 12 处（value.rs 11 + domain.rs 1）全部属 CR-001/CR-002 批准载体，非克隆/构造路径 → **B 档重跑前置门解除** | 代码审计（value.rs/model.rs/domain.rs） |

**机时记账**：Phase 0 实耗 ≈4 次有效运行 + 2 次亚秒级失败探测（6/≈7 次预算），机器时间 ≈20 分钟。**方案收缩汇总**：Tier 1.2 裁撤；Tier 1.1 降级为 W3-3/W3-4 配套动作（eq 族按 kill criteria 以首个数据点提前路由 Phase 2 stub 试点）；Tier 2.3 组合验证两步化。Phase 1 计划不变（W3-1 结构自检断言先行）。

### 3.8 Phase 1 W3-1 执行记录（结构自检断言，2026-09-14 定稿）

**落地内容**（`tests/kani/kani_proofs.rs` 单文件，proof 源码变更）：

1. **7 个结构自检助手**：`shape_field`（键存在+返回引用）/ `shape_str`（字符串相等）/ `shape_str_in`（集合哨兵）/ `shape_array`（定长数组）/ `shape_bool` / `shape_payload_leaf`（payload 叶子定位）/ `shape_full_state` 与 `shape_concrete_exec_state`（两族 state 哨兵）。全部为具体值相等断言——不引入符号分支、不改变被证属性解空间，只拦截构造退化导致的假验证。
2. **23 个 B 档 harness 全部接线**（含 13 处构造根接线修复：原草稿将 `payload` 键误作构造根传入 `shape_payload_leaf`，按各 harness 实际构造根改接）。
3. **canary 本地验证（验证后删除，不入库）**：
   - 正向 ✅：`c1`（shape_field/shape_str/shape_str_in，76.9s）、`c3`（shape_payload_leaf × single_key_exec_state，32.5s）、`c2b`（shape_array 裸数组最小载体，**0.55s**）；
   - 反向 ✅：`degraded_fails_loudly`（F1 类退化构造）13.9s 于预期断言点精确响亮失败（unwind 8 下验证，其失败为键缺失 panic，与 unwind 取值无关）——假通过防线成立；
   - **构造墙发现（移交 W3-3/W3-4）**：全具体构造在 CBMC 0.67 下随构造复杂度非线性恶化——`c2`（2 键 map+1 元数组，断言逻辑与 c2b 完全相同）300s 不收敛 vs `c2b`（裸数组）0.55s；`c4a`（`concrete_exec_state` 纯构造、零断言）120s 不收敛；`c5`（3 层嵌套镜像 state）约 250s 被终止。**构造成本本身（String/Cow/Vec/分配器建模）是膨胀源，与断言无关**（T0-6 结论在微型尺度复现）。`shape_full_state`/`shape_concrete_exec_state` 两复合哨兵的载体构造受同一构造墙限制而无法独立实跑，由「原语已验证 + 具体相等断言 + 反向防线」支撑，完整实跑验证随 W3-3/W3-4 闭环。**连带影响**：P9/P10（构造 `concrete_exec_state`）Phase 1 直跑将撞同一构造墙，须 W3-3 owned 迁移或 W4-1 stub 路线先解除。
   - **调试插曲（两层根因，W3-2 要求更新）**：整体 canary 于 unwind(8) 两次 600s 不收敛曾疑似求解器问题；`--debug` 探针 + 拆分定位还原真因：① `unwind(8) < memcmp 字节循环深度`（键/值串如 `"payload.x"` 9 字符需约 10 次展开）→ unwinding 断言失败毒化公式（920 项检查 919 项 undetermined）→ 求解器无限研磨（非求解器问题）；② 修至 unwind(24) 后露出上述构造墙。**W3-2 配套要求据此更新**：B 档 harness 的 unwind 必须 > 其形状断言最长字符串的 memcmp 深度（字节数+2），否则 unwinding 断言假失败。

**M3.4 证据处理**：kani_proofs.rs 变更使 A 档 14 proof 的 `1c6ad84` 证据 SHA 绑定失效 → 提交后 WSL 同协议重跑 14 个，新证据 `P0-3/P0-6.<harness>_PASS_<新SHA>_20260914_*` 落盘，旧 14 对 `git mv` 隔离 `_invalidated/` 批次 3；STATUS.md 证据列/快照同批更新（另见 DISCLOSURE_LOG 同日条目）。

**机时记账**：canary 全程 ≈17 次运行（含 2×600s 毒化研磨、300s+250s+2×120s 构造墙实证、4 次有效 PASS、探针 2 次），机器时间 ≈60 分钟；B 档攻坚累计 ≈24/≤60 次预算。**W3-1 结论：S3 硬前置达成**。

## 4. CR-20260913-003 修订记录（2026-09-13，随 CR-20260913-004 生效）

**修订**：实施载体由 impl 级 `cfg(kani)` 双实现改为 proof 层
`#[kani::stub(...)]`（依据 [ADR-0002](../docs/adr/ADR-0002-B档proof模型载体与stub化验证策略.md)）。

**修订理由（事实）**：载体复核确认 impl 级覆写在 kani 构建下影响
全部调用点（含 harness 构造层 `object_from_pairs` 内的 `v.clone()`，
嵌套复合输入在深度 1 处被清空——proof 实际验证退化输入）；且真实
trait impl 从 kani 构建产物中消失，使"真实现侧证明补偿"的登记
（如 eq 模型挂 A 档 `verify_partial_eq_never_panics`）引用到被覆写
的模型本身，补偿不可成立。

**修订动作**：value.rs 两处 impl 级 cfg(kani) 覆写（Clone/PartialEq）
回退；原模型语义（容器变体长度比较/空容器拷贝、标量保真）保留为
stub 模型设计输入，由 proof 层声明承载。原 CR 文本（下文）保留不删，
其中 §3.2/§3.4/§3.5 所述 impl 级实现方式以本修订记录为准。

**原批准内容如下**（未改动）：

---

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260913-003 |
| **变更标题** | JsonValue PartialEq/Clone Kani 模型化：容器变体长度比较/空容器拷贝，消除 eq 分支 CBMC 编码爆炸（B 档 proof 第三层根因） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-13 |
| **审查状态** | 已批准（2026-09-13 载体修订：见上方修订记录） |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
round 9 三变体探针（同一 evaluate_eq 副本，各 stub 一个嫌疑，
其余保真，unwind 24）实测全部 150s 超时：
- probe3a_noclone（去 clone，真 == + 真 resolve）
- probe3b_noresolve（去 resolve，真 clone + 真 ==）
- probe3c_nocmp（去 ==，真 clone + 真 resolve）
两两交集只剩 == 与 clone 各自独立爆炸：
- 手写 PartialEq 的 Array 分支 `Vec ==`（符号长度按 unwind 上限
  展开，元素递归 JsonValue 比较）
- 手写 PartialEq 的 Object 分支 `iter`/`get` 逐键循环 + 递归 `!=`
- derive(Clone) 的 Vec/ObjectMap 递归拷贝（同符号长度展开）
对照：probe2_q（全 stub 分支）35s PASS；A 档 P1/P2 对具体小值
的 ==/cmp 9~28s PASS（常量传播限制路径，无符号容器展开）。
本变更将 PartialEq/Clone 改为 impl 级 cfg 双实现：
- cfg(not(kani))：原手写 PartialEq 逐字保留；Clone 手写全变体
  深拷贝（与 derive 等价）
- cfg(kani)：容器变体 == 只比长度、clone 返回空容器；标量变体
  （Null/Bool/Integer/String-Cow）保持真比较/真拷贝
```

### 3.1 变更理由

CR-001（KaniMap）+ CR-002（递归树有界化）落地后 eq proof 仍超时
（MAX_DOMAIN_DEPTH=1 实测 300s），第三层根因为 eq 分支内部的
`==`/`clone` 符号容器编码爆炸。

### 3.2 变更范围

- value.rs：`impl PartialEq`（impl 级 cfg 双实现）；`#[derive(Debug, Clone)]`
  改 `#[derive(Debug)]` + 手写 `impl Clone`（impl 级 cfg 双实现）

### 3.3 破坏性分析

无。生产构建（not(kani)）PartialEq 原实现逐字保留，手写 Clone 与
derive 语义等价（全变体深拷贝）；kani 构建的模型语义在下方披露。

### 3.4 影响评估

- 属性安全性：B 档 proof 对求值结果均 `let _ =`（never-panic），
  不断言值语义；A 档 P1（PartialEq 永不 panic）同样 `let _ =`
- reactor proof 断言全为标量（usize/bool），无 JsonValue == 依赖
- kani 下 eq 域值语义弱化（容器变体只比长度）如实披露；
  值语义由 cargo test（非 kani 构建）与 differential 测试覆盖

### 3.5 测试计划

- 全 workspace `cargo test` 回归（生产 Clone/PartialEq 等价性）
- cfg(kani) `cargo check` 编译验证
- eq 系 proof Kani 实测（验证编码爆炸消除）

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260913-002 |
| **变更标题** | 域评估递归树有界化：cfg(kani) 下 MAX_DOMAIN_DEPTH 64→4，消除 CBMC 递归调用树指数编码（B 档 proof 第二层根因） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-13 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
Kani 0.67 CBMC 后端对 evaluate_domain_inner 的 all/not 分支递归
调用自身做无条件调用树编码（每层扇出 2）：unwind 24 = 2^24 实例，
150s 超时且与运行时输入无关。2026-09-13 六轮二分探针定位：
- probe2_q（同逻辑去递归版，all/not 换非递归 stub）35s PASS（unwind 24）
- probe2_o（真递归版，相同 unwind）151s TIMEOUT
- 全内联等价物 / 本地函数跨调用（含 ok_or_else 闭包 + ? 传播
  + Result 跨函数返回）均 13-23s PASS
本变更在 cfg(kani) 下将 MAX_DOMAIN_DEPTH 从 64 降为 4：
- 深度保护分支（depth > MAX_DOMAIN_DEPTH 返回 Err(NestingTooDeep)）
  使递归树有界终止（2^5 实例），无需 unwind 截断
- 生产构建（cfg(not(kani))）保持 64 零改动
```

### 3.1 变更理由

B 档 23 个 proof 中域评估系列（P8a-P8g/P9/P10）在 KaniMap
（CR-20260913-001）落地后仍超时——存在独立于 BTreeMap 的第二层
根因：递归调用树指数编码。

### 3.2 变更范围

- domain.rs：`MAX_DOMAIN_DEPTH` 条件编译（`not(kani)`=64 / `kani`=4），
  文档注明 Kani 验证模型语义（「深度限制为 4 的域评估器」完备验证）
- tests/kani/kani_proofs.rs：P10 适配（构造 `MAX_DOMAIN_DEPTH+1` 层 not、
  unwind 70→12、常量引用替代硬编码 65、import MAX_DOMAIN_DEPTH）

### 3.3 破坏性分析

无。生产构建 `MAX_DOMAIN_DEPTH=64` 零改动；kani 构建下深度限制
变化已在 proof 属性语义中如实披露。

### 3.4 影响评估

- 全 workspace `cargo test` 回归须绿（生产路径无改动）
- kani 构建下 src 单元测试仅编译不运行（65 层构造测试无影响）
- P10 属性语义更新：验证深度保护分支在 5 层到达且不 panic

### 3.5 测试计划

- [x] 代表性 proof 探针定位（六轮二分，根因 100% 确认）
- [ ] `cargo check`/`cargo test` 生产构建回归
- [ ] 代表性 B 档 proof Kani 实测（eq + P10）
- [ ] 23 个 B 档 proof 全量重跑 + A 档 14 个 + reactor 4 个回归

### 3.6 回滚方案

git revert 本提交即恢复 MAX_DOMAIN_DEPTH=64 单值定义（B 档域评估
proof 回到不可运行状态，无其他副作用）。

---

## 附 · 历史变更归档

### CR-20260913-001（已批准）：ObjectMap Kani 后端切换：cfg(kani) 下 KaniMap（有序 Vec）替代 BTreeMap，消除 23 个 B 档 proof 状态爆炸

> 归档说明：原 CR 整表置顶至 2026-09-13（CR-20260913-002 置顶），与 002 同批次提交入库，完整内容见 git 历史。要点：kani 构建下 ObjectMap=KaniMap（有序 Vec 模拟 BTreeMap，条目键字典序不变，API 兼容），JsonValue/KaniMap 增 cfg(kani) no-op Drop；生产构建零改动。
### CR-20260902-001（已批准）：元指令类型白名单 SSOT 化：META_INSTRUCTION_TYPES 常量导出 + 漂移防线（UV-046 C2）

> 归档说明：原 CR 整表置于顶层至 2026-09-13（CR-20260913-001 置顶），完整内容见 git 历史。

### CR-20260830-001（已批准）：build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除）

> 归档说明：原 CR 整表置于顶层至 2026-09-02（CR-20260902-001 置顶），完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260830-001 |
| **变更标题** | build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-30 |
| **审查状态** | 已批准 |

本变更只修改 build.rs 门禁自身实现，不触及任何 src/ 执行语义：
strip_test_mod/find_inline_lbrace/match_brace 状态机在撇号处新增
char_lit_starts() 判别（字符字面量 vs 生命周期），新增 skip_lifetime() 跳过；
修复前 'static 等生命周期撇号被误判为字符态开头，令 tests 模块整体不被剥离、
门禁对测试代码全量误报。五仓同一份实现同步修复。回滚：git revert。

### CR-20260827-001（已批准）：core_eval.json v0.4.0：ReAct 应用剧本整体迁出至消费方（T8 最小化专项）

> 归档说明：原 CR 整表收录于 2026-08-30，完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260827-001 |
| **变更标题** | core_eval.json v0.4.0：ReAct 应用剧本整体迁出至消费方（T8 最小化专项） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-27 |
| **审查状态** | 已批准 |

资产层变更：core_eval.json 移除三条 ReAct 循环 transform 规则（v0.3.1 → 0.4.0），剧本迁至消费方自持运行宪法（app.evoagent.agent v0.4.0），核心仓回归最小引擎自评估集；机制层代码零改动。回滚：git revert 即恢复 v0.3.1。

### CR-20260820-002（已批准）：添加变更治理门禁机制和策略层检测

> 归档说明：原文件整表收录于 2026-08-27，内容未改动。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260820-002 |
| **变更标题** | 添加变更治理门禁机制和策略层检测 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-20 |
| **审查状态** | 已批准 |

本次变更提供通用的变更治理基础设施：CHANGE_REQUEST.md 验证是通用的审查流程管理能力；策略层反模式检测是通用的代码质量保障能力；这些能力可被任何机制层代码复用；不包含任何特定业务语义；定义的是"怎么做"的通用方式，而非"做什么"的业务规则。属机制层变更，影响 build.rs 与本文件自身。回滚方案：删除 build.rs 中的变更治理验证代码和策略检测代码即可。
