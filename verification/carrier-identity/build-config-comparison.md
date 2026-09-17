# 载体同一性 · 构建配置比对（DEV-6 · 证据形态 A）

> **证据类型**：AL4 判定条件（[ASSURANCE.md](ASSURANCE.md) §2.2 / §8.1）所要求的「载体同一性证据」之**静态比对**半部。
> **配套半部**：差分验证（DEV-6 证据形态 B）见同目录 `differential-verification.md`，由本地 `cargo test` 执行并归档输出。
> **关联偏离**：[STATUS.md](STATUS.md) DEV-6（全部 AL4 目标）；DEV-1（分叉消除，长期路径）。
> **本文件性质**：状态/证据记录（M1），**非规范性文件**。不改变任何 ASSURANCE.md 声明。

---

## 0. 证据元数据（ASSURANCE §8.1）

| 字段 | 值 |
| --- | --- |
| 验证对象标识（commit SHA） | `d9ceb9c3fda8a4a59f2fc4dc28824730bf156382`（2026-09-17 19:49 +0800） |
| 生产构建工具链 | workspace `rust-toolchain.toml` → `1.97.1`（stable，profile=minimal，rustfmt+clippy） |
| 验证构建工具链 | `evorule-tcb/rust-toolchain.toml` → `nightly-2025-11-21` + **Kani 0.67.0** |
| 验证触发 | `cargo kani` 自动注入 `--cfg kani`（reactor 经 `src/pure.rs` `#[cfg(kani)]` 挂载 proof 模块；tcb 经 `tests/kani_entry.rs`） |
| panic 策略 | 默认（unwind） |
| profile | 生产二进制 = release；Kani harness 不走 release profile（独立翻译层） |
| feature 组合 | 默认集；`persistence`（facts_log WAL）、`ffi`（gated unsafe）按 crate 启用；`kani` 非 cargo feature，由工具链注入 |
| 界声明 | 见各 proof 的 `--default-unwind` / 显式 `unwind` 注解（tcb 23 处文件内 `#[kani::unwind]` 不可统一覆盖） |

---

## 1. 比对范围与方法

**比对对象**：同一份源码在 `cfg(not(kani))`（生产）与 `cfg(kani)`（验证）两条编译路径下的**行为级差异**。

**枚举口径**：全仓 `*.rs` 中 `#[cfg(kani)]` / `#[cfg(not(kani))]` 命中点，剔除「仅注释提及」与「仅挂载 proof harness（非生产路径分叉）」两类**非分叉**项，得到 **13 处实质性分叉（D1–D13）**。

**判定词汇**（与 ASSURANCE §2.2 对齐）：
- **行为等价**：可观察输出在相关输入域一致，替身可在 AL4 意义上视为生产载体。
- **保真损失**：替身与生产在可观察行为上**存在已知差异**，直接命中 AL4 禁令「无 `cfg` 分叉替换被验证的行为 / 无仅在验证配置下生效的常量改写 / 无可观测行为差异」——该处验证结论**不得**晋升 AL4。
- **新增字段**：替身专属内部状态，不影响生产路径被验证行为。
- **非分叉**：仅挂载验证代码或注释，不替换生产行为。

---

## 2. 分叉清单（逐条比对）

### 2.1 `evorule-tcb/src/value.rs`

| 编号 | 位置 | 生产（`cfg(not(kani))`） | 验证（`cfg(kani)`） | 判定 |
| --- | --- | --- | --- | --- |
| **D1** | L199–203（`ObjectMap` 类型别名） | `BTreeMap<String, JsonValue>` | `KaniMap`（有序 `Vec` 后端，`Vec<(String,JsonValue)>`） | **行为等价（有论证）** |
| **D2** | L266–277（`JsonValue` Drop） | 默认析构（递归字段 drop） | no-op Drop（`mem::take`+`forget` 跳过元素析构） | **行为等价（析构无副作用）** |
| **D3** | L281–292（`null_ref`） | `&JsonValue::Null` | `&NULL_VALUE`（同值静态，规避 E0515） | **行为等价** |

**D1 论证**：`KaniMap` 维护键字典序（`insert` 维持），`iter()` 迭代序与 `BTreeMap` 一致；API 与在用方法（`get`/`get_mut`/`insert`/`remove`/`contains_key`/`values`/`iter`）签名兼容；`JsonValue` 析构无副作用，no-op Drop 不影响可观察输出。被验证属性（路径解析、JsonValue 构造/访问子集）在替身上成立 ⇒ 在生产载体上同成立。

### 2.2 `evorule-tcb/src/domain.rs`

| 编号 | 位置 | 生产 | 验证 | 判定 |
| --- | --- | --- | --- | --- |
| **D4** | L43–47（`MAX_DOMAIN_DEPTH`） | `64` | `1` | **保真损失（常量改写）** ⚠️ |

**说明**：常量在验证配置下被改写（深度上界 64→1）。这是 **AL4 禁令「无仅在验证配置下生效的常量改写」的直接命中**，也是 C2 被规则 R1 封顶 AL2 的核心证据。域求值证明仅覆盖深度 1；生产允许深度 64，验证结论**不主张**深度 ≥2 的域求值无未定义失败。

### 2.3 `evorule-reactor/src/state.rs`

| 编号 | 位置 | 生产 | 验证 | 判定 |
| --- | --- | --- | --- | --- |
| **D5** | L182–230（4 个字段 + L272–292 构造器） | `BTreeSet<FactId>` / `BTreeMap<FactId, …>` | `KIdSet` / `KIdMap`（Vec 线性，源码自述「**不保证迭代顺序**」） | **保真损失（迭代序不保证）** ⚠️ |
| **D6** | L254–259 / L291（`kani_has_io_result`） | 无此字段 | kani 专属 `bool` 标志 | **新增字段（kani 专属）** |
| **D7** | L367–371（`clear_queue`） | `clear()`（drop 元素） | `mem::take`+`forget`（跳过元素析构） | **行为等价（跳过析构）** |
| **D8** | L516–520（`clear_io_result`） | 写完整 I/O 结果状态 | 仅置 `kani_has_io_result=false` 即返回（**不写 payload**） | **保真损失（不写 payload）** ⚠️ |

**D5 说明**：`KIdSet`/`KIdMap` 替代红黑树是为绕开 CBMC 状态爆炸，但代价是**放弃确定迭代序**。对依赖确定迭代序的属性（**C1 执行确定性**）构成保真损失——除非被验证属性显式不依赖该集合的迭代序。

### 2.4 `evorule-reactor/src/pure.rs`

| 编号 | 位置 | 生产 | 验证 | 判定 |
| --- | --- | --- | --- | --- |
| **D9** | L271–275（`inject_io_result`） | 写完整 `result` payload 入状态 | `let _ = (result, io_type)` 丢弃结果，仅置标志位 `kani_has_io_result=true` 返回 | **保真损失（不写 payload）** ⚠️ |
| **D10** | L358–360（proof 模块挂载） | —（无此模块） | `#[cfg(kani)] #[path=...] pub mod kani_proofs;` | **非分叉（验证代码挂载）** |

**D9 说明**：与 D8 同源——替身下 I/O 结果注入**不实际存储 payload**，仅置标志。被验证的「inject 不 panic」结论成立，但「inject 后状态含该 payload 且可被后续读取」这一行为级性质未被替身覆盖。

### 2.5 `evorule-reactor/src/hash.rs`

| 编号 | 位置 | 生产 | 验证 | 判定 |
| --- | --- | --- | --- | --- |
| **D11** | L292–（`content_hash`）/ L340–（`fact_hash`）/ L481–（`chain_step`） | `blake3` 密码学哈希 | 确定性简化哈希（单字符映射 `a–g` / 字节拼接，`String::from`+`push_str` 规避 `format!` 状态爆炸） | **结构性质等价；密码学性质不在覆盖内** |

**说明**：简化哈希保持**幂等性**（同输入同输出）与**区分性**（不同输入不同输出）。被验证的「链构造单射 / 篡改必破坏链」**结构性质**在两者皆成立。密码学抗碰撞性本属假设 H3.1，不在任何等级覆盖内，故不构成 AL4 缺口。

### 2.6 `evorule-reactor/src/facts_log.rs`

| 编号 | 位置 | 生产 | 验证 | 判定 |
| --- | --- | --- | --- | --- |
| **D12** | L158–169（`FactsLogLock`） | `RwLock<FactsLogInner>`（多读取者并发） | `RefCell<FactsLogInner>` + `unsafe impl Sync`（kani 单线程，仅为满足编译期 `Send` 检查） | **保真损失（并发模型替换）** ⚠️ |
| **D13** | L663–（`append` 简化路径） | 完整路径：哈希链更新 + WAL 写盘 + 内存状态 + 快照/队列更新 | 跳过 `last_hash` 更新、跳过 `current_snapshot`/`current_queue` 更新（仅 `version` 递增） | **保真损失（append 路径大幅简化）** ⚠️ |

**D12 说明**：生产用真实 `RwLock` 提供并发安全；替身用 `RefCell` + `unsafe impl Sync`，其线程安全**仅由 Kani 单线程假设保证**，不代表生产并发行为。对并发相关声明（隐含于 C1 确定性 / C5 重放）为保真损失。
**D13 说明**：替身 `append` 仅验证「version 递增 + 少量字段」，跳过了哈希链与快照/队列更新——被验证的 append 性质对应的操作远小于生产 append。

---

## 3. 非分叉项（明确排除，避免误报）

| 位置 | 内容 | 性质 |
| --- | --- | --- |
| `evorule-tcb/src/executor.rs:618` | 仅注释提及 `cfg(kani)` no-op Drop | 注释，无分叉 |
| `evorule-reactor/build.rs:311` | T10 豁免文档注释提及 `facts_log.rs` 的 `#[cfg(kani)]` `unsafe impl Sync` | 注释，无分叉 |

---

## 4. 汇总与 AL4 判定结论

### 4.1 分叉归类计数

| 判定 | 数量 | 编号 |
| --- | --- | --- |
| 行为等价（含结构等价） | 5 | D1、D2、D3、D7、D11 |
| **保真损失** | **6** | **D4、D5、D8、D9、D12、D13** |
| 新增字段（kani 专属） | 1 | D6 |
| 非分叉 | 1 | D10 |

### 4.2 对 AL4 目标的阻断结论（诚实）

- **C2 无未定义失败（目标 AL4/AL3）**：存在 **D4（常量改写 64→1）** 这一 AL4 直接禁令命中项，且 D5/D8/D9/D12/D13 为行为级保真损失。⇒ **C2 在 tcb/reactor 载体上无法达到 AL4**，与 STATUS 当前「C2 = AL2（受 R1 封顶）」一致。
- **C1 执行确定性（目标 AL4）**：D5（迭代序不保证）、D12（并发模型替换）直接威胁确定性声明。⇒ **C1 在当前载体下无法达到 AL4**。
- **C6 守卫强制（目标 AL4）**：守卫逻辑位于 `evorule-governance`，**该 crate 无任何 `cfg(kani)` 分叉**（见全仓枚举：分叉仅存在于 tcb/reactor）。⇒ **C6 的载体即生产载体，DEV-6 对其不构成阻断**；C6 的 AL4 可行性取决于 C6.2（`pub` 写路径穷尽证据，当前 AL2 缺口）与 C6.4/C6.5 运行时观测的闭合，而非载体同一性。

### 4.3 关闭 DEV-6 的两条路径（供决策）

1. **消除分叉（DEV-1 路线，长期）**：将 D4–D13 的替身回退为生产载体，使验证在生产配置下直接运行（需解决 CBMC 对 BTreeMap/blake3/并发 的状态爆炸——即形式化根因审计确立的「构造墙」，当前判定为原理级死结，须借 TLA+/类型系统分流）。
2. **边界化 + 差分验证（务实路线）**：保留替身，但对每处保真损失**显式界定**其影响域（哪条被验证属性、在哪个输入子集上仍等价），并以**差分验证（证据形态 B）** 在生产载体上实证该属性成立；归档后 DEV-6 视为「已建立证据形态」但相关 AL4 声明仍受边界约束。

> 无论走哪条，本文件（证据形态 A）+ 差分验证输出（证据形态 B）构成 DEV-6 要求的**证据形态本身**。当前状态：**A 已建立，B 待本地执行归档**；DEV-6 标记维持「未关闭」直至 B 产出且保真损失项完成边界化或消除。

---

## 5. 下一步（证据形态 B · 差分验证）

见同目录 `differential-verification.md`：在生产载体（`cfg(not(kani))`）上以有界代表域 proptest 实证下列被验证属性，捕获输出归档：

1. `JsonValue` 构造/访问/序列化在真实 `BTreeMap` 下 no-panic + 确定性（对应 D1–D3 的等价论证）。
2. 域求值在 `MAX_DOMAIN_DEPTH=64` 下有界、no-panic（对应 D4 边界化）。
3. reactor 状态在 `BTreeSet/BTreeMap` 下确定性迭代（对应 D5）。
4. `inject_io_result` / `clear_io_result` 在真实载体下写入并可读回 payload（对应 D8/D9）。
5. `content_hash`/`fact_hash`/`chain_step` 在 `blake3` 下仍满足链构造单射 + 篡改必破坏链（对应 D11）。
6. `append` 在真实 `RwLock` + 完整路径下 version 递增 + 快照/队列一致（对应 D12/D13）。
