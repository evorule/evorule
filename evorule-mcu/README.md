# evorule-mcu

EvoRule 嵌入式（MCU）no_std 内核：紧凑规则包 + RAM WAL + tick 时钟 + 同步执行器。

- `no_std` + `alloc`，无 std 依赖；审计链哈希经 `evorule-audit-chain` 单一真相源，
  与 std 宿主（reactor）同输入产出逐字节一致的 WAL 行与链尾哈希。
- 紧凑规则包（`.evopack`）：postcard 编码的 core_eval 规则数组（host 编译器
  `evorule-cli` 负责生成）。
- RAM WAL：`FactWalStore` 同构语义（append-only、哈希链字段原样保存、
  write-ahead 先于状态更新）。
- 同步执行器：事件（指令）→ `execute_transition` → 审计（StateTransition /
  TransitionTrace / Stable 等事实发射语义与 reactor 主循环一致）→ RAM WAL。
- `io_request` 元指令：暂停-恢复协议——IoRequest 事实入链后 `run` 返回
  `McuError::IoPaused`；板侧经 `IoBridge` 执行 I/O 后调 `resume_io` 继续
  （结果注入 `__io_results__.{io_type}` → 原指令重推 → 恢复步清除容器，
  镜像 reactor v0.3.1 I/O 恢复语义）。
