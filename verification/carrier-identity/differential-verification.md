# 载体同一性 · 差分验证（DEV-6 · 证据形态 B）

> **配套文件**：[build-config-comparison.md](build-config-comparison.md)（证据形态 A，静态比对，已建立）。
> **执行方**：维护者执行并归档输出。
> **目的**：在生产载体（`cfg(not(kani))` = 真实 `BTreeMap` / `MAX_DOMAIN_DEPTH=64` / `blake3` / `RwLock`）上，以有界代表域实证「被验证属性同样成立」，弥补替身（cfg(kani)）的行为级偏差，构成 AL4 载体同一性证据的实证半部。

---

## 1. 为什么需要它（对齐 ASSURANCE §2.2）

AL4 = AL3 + 「证明所覆盖的代码在生产构建配置下与交付物逐字等同」。Kani proof 跑在替身（`#[cfg(kani)]`）上；差分验证的作用不是「在替身上再跑一遍 Kani」，而是**在生产载体上用属性测试证明同一性质成立**——从而把「替身结论」桥接到「生产载体」。

> **关键限定**：差分验证是 AL1 证据（抽样/证伪，非机器核验证明），它**不能单独**把某条声明抬到 AL4；它的角色是「载体同一性的实证桥接 + 保真损失项的边界化」。与 A 部分（静态比对）合并，才算 DEV-6 证据形态齐备。

---

## 2. 承载方式（把 harness 放在哪）

- **首选**：各 crate 既有测试目录新增 `#[cfg(test)] mod carrier_diff`，或放入 `verification/carrier-identity/diff_harness.rs` 经 `#[path]` 挂载（参照 `evorule-reactor/src/pure.rs:358` 的挂载风格）。
- **必须 `#[cfg(not(kani))]` 守卫**：差分 harness 只在生产载体编译运行，绝不进入 Kani 翻译层（否则又跑回替身）。
- 每个测试以 `proptest!` 驱动，**有界**代表域（深度 ≤ 64、键数 ≤ N、fact 数 ≤ N），并在文档注明域边界（对应 R2 的「界必须显式声明」）。

---

## 3. 测试清单（映射到 D1–D13）

| # | 对应分叉 | 测试目标（生产载体上） | 断言 |
| --- | --- | --- | --- |
| T1 | D1–D3 | `JsonValue` 构造 / `ObjectMap` 访问 / 序列化 | 任意有界输入 no-panic；同输入 `to_string` 确定性；`BTreeMap` 序与无替身时一致 |
| T2 | D4 | 域求值 `evaluate_domain`（`MAX_DOMAIN_DEPTH=64`） | 深度 ≤ 64 有界、no-panic；深度 > 64 显式拒绝（不静默通过） |
| T3 | D5 | reactor 状态集合迭代（`BTreeSet/BTreeMap`） | 同插入序下 `iter()` 输出确定（与替身「不保证迭代序」形成对照，界定 C1 影响域） |
| T4 | D8/D9 | `inject_io_result` / `clear_io_result`（真实载体） | 注入后状态含 payload 且 `has_io_result` 可读回；clear 后 payload 不可见 |
| T5 | D11 | `content_hash` / `fact_hash` / `chain_step`（`blake3`） | 链构造单射（`chain_step(h1,c)≠chain_step(h2,c)` when h1≠h2）；篡改内容必改链；幂等 |
| T6 | D12/D13 | `FactsLog::append`（真实 `RwLock` + 完整路径） | `version` 单调递增；append 后快照/队列与内存一致；多读取者并发下无数据竞争（单测并发即可；若引 loom 须新增 dev-dependency） |

> T1–T6 的**失败**即证明该处保真损失为「真实行为差异」→ 必须在 A 文件 §4.3 路径二下**显式边界化**（标注影响的属性 + 输入子集），相关 AL4 声明维持受限；**通过**则桥接成立。

---

## 4. 运行命令（本地）

```bash
# tcb 载体差分（T1、T2）
cargo test -p evorule-tcb carrier_diff

# reactor 载体差分（T3、T4、T5、T6）
cargo test -p evorule-reactor carrier_diff

# governance 守卫（C6 无 cfg(kani) 分叉，直接 AL4 路径证据）
cargo test -p evorule-governance permission
```

> 无需额外 cfg 旗标：`#[cfg(not(kani))]` 在 stable 工具链下恒真（`kani` cfg 仅由 Kani 工具链注入），普通 `cargo test` 天然只编译生产载体。前置检查用 `cargo rustc -p evorule-tcb -- --print cfg`（或 `cargo tree`）确认输出不含 `kani`。

---

## 5. 输出归档格式（使 B 成为可审计证据）

每次运行捕获并重命名为：

```
verification/carrier-identity/evidence/
  DIFF-T1_T2_<commit>_<YYYYMMDD_HHMMSS>.log   # tcb 载体差分
  DIFF-T3_T6_<commit>_<YYYYMMDD_HHMMSS>.log   # reactor 载体差分
  DIFF-C6_<commit>_<YYYYMMDD_HHMMSS>.log      # governance 守卫
```

归档头须含 §8.1 元数据：`commit SHA`、`rustc --version`、`cargo --version`、命令、用例数、PASS/FAIL、耗时。归档后于 [STATUS.md](STATUS.md) 维护区登记「DEV-6 证据形态 B 已归档（<路径>）」，并将 DEV-6 处置方向更新为「已建立证据形态；保真损失项边界化状态 = …」。

---

## 6. 与 DEV-1 的关系（避免重复劳动）

- **DEV-1** = 消除分叉（长期，受构造墙约束，当前判定原理级死结）。
- **DEV-6（本文件）** = 即使分叉未消除，也要有「构建配置比对 + 差分验证」**证据形态**归档，否则 AL4 不可判。
- 二者不冲突：先以 DEV-6 把证据形态补齐（A 已建、B 待跑），DEV-1 作为长期工程在 TLA+/类型系统分流后推进；DEV-1 完成时分叉减少，B 的保真损失项同步收敛。
