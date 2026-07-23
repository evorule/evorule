Tier-1（治理层/反应器）和 Tier-2（I/O 处理/API）的编程规范，严格遵循 **`Governance实施防漂移标准.md`**，并由 `governance_core_build.rs` 在编译期强制检查。

这套规范的核心试金石是**“机制-策略分离原则”**：

> **如果业务需求变了，这行代码需要改吗？**
>
> - **需要改** → 这是**策略**（业务逻辑），**必须**放在 JSON 数据中。
> - **不需要改** → 这是**机制**（执行框架），**允许**写在 Rust 中。

基于此原则，具体到代码层面，划分非常明确：

---

### ✅ 允许在 Rust（治理层/反应器）中做的事情（“机制”）

这些属于系统的“骨架”或“管道”，不包含业务意图，允许编写 Rust 代码：

1. **流程编排与路由**（`orchestrator.rs`, `io_dispatcher.rs`, `server.rs`）：
   - 拆分事件队列、调用 TCB 单步执行、接收 Fact 并分发。
   - 将 HTTP 请求体解析为 `Fact::Command`（仅做格式转换，不做业务判断）。
   - 根据 `IoType` 枚举路由到具体的 Handler（路由是机制，路由的目标是策略）。
2. **数据加载与结构转换**（`rule_loader.rs`, `compiler.rs`, `FactsLog`）：
   - 从文件系统读取 JSON 并反序列化。
   - 将 JSON 规则树转换为内部指令结构（纯粹的格式映射，无校验逻辑）。
   - 追加式日志的读写、版本号的单调递增（日志存储是机制）。
3. **审计与哈希框架**（`auditor.rs`, `hash.rs`, `clock.rs`）：
   - 记录 TCB 返回的 `before`/`after` 快照（只记录，不判断内容）。
   - 计算 BLAKE3 哈希、维护逻辑时钟（审计工具是机制）。
4. **反应器生命周期控制**（`reactor.rs`, `stable_detector.rs`）：
   - 控制 `max_rounds` 循环、检测版本号是否变动（循环控制是机制）。
   - **注意**：稳定检测的逻辑（版本号比较）是纯算法机制，但**稳定阈值（如 3 次）** 是策略，必须来自 JSON 配置，不得硬编码。
5. **纯 I/O 传输**（`llm_handler.rs`, `db_handler.rs`）：
   - 接收 `params`（来自 `Fact::IoRequest`），构造标准的 HTTP/TCP 请求发送出去。
   - 接收网络响应，原样封装为 `Fact::IoResponse` 返回。

---

### ❌ 绝对禁止在 Rust 治理层做的事情（“策略”）

这些是业务逻辑或业务数据，一旦写在 Rust 中即构成“漂移”，`build.rs` 会直接拦截编译：

1. **硬编码业务指令类型或阈值**（违反 F1, F2）：
   - ❌ `if instruction_type == "math_rule" { ... }`
   - ❌ `if score > 80 { ... }`（阈值必须在 JSON 中）
   - _例外_：`io_dispatcher.rs` 中的 `match io_type { IoType::CallLlm => ... }` 是允许的（这是路由机制，不是业务判断）。
2. **拼接动态字符串模板**（违反 F3, F4）：
   - ❌ `format!("请总结：{}", content)` （Prompt 模板属于业务策略）
   - ❌ `format!("SELECT * FROM users WHERE id={}", id)` （SQL 语句必须参数化，来自 JSON）
3. **硬编码权限或角色判断**（违反 F5）：
   - ❌ `if user.role == "admin" { ... }`（权限映射必须在 `auth_config.json` 中）
4. **在 Rust 中对规则列表进行过滤或排序**（违反 F6）：
   - ❌ `rules.iter().filter(|r| r.type == "active").collect()`
   - _允许_：读取文件后按文件名排序（为确保确定性），但业务过滤条件必须在 JSON 中。
5. **包含业务术语的字符串字面量**（违反 §5.2 黑名单）：
   - ❌ `"math_rule"`, `"physics_rule"`, `"admin"`, `"teacher"`, `"summarize"`, `"call_external"` 等出现在 Rust 字符串中（除非是 dispatcher 的枚举匹配）。
6. **复杂的嵌套逻辑或长函数**（违反 F8, F9）：
   - ❌ `if/else` 嵌套超过 **2 层**。
   - ❌ 单个函数超过 **50 行**（暗示内部嵌入了业务逻辑）。
7. **跨 Handler 相互调用**（违反 F10）：
   - ❌ `llm_handler` 调用 `db_handler` 的方法（Handler 必须独立，只负责自己的 I/O 类型）。
8. **使用 `debug_assert!`, `unwrap()`, `expect()`**（违反 F11 & 安全要求）：
   - ❌ 这些会导致 Panic 或 Debug/Release 行为不一致，必须使用 `?` 操作符返回确定性的 `Err`。

---

### 🆕 V5.0 架构下的特别补充（反应器模式）

在最新的 **V5.0（反应式数据执行器）** 架构中，上述规范依然**完全适用**，且新增了以下映射：

- **允许（机制）**：编写 `ReactiveEngine` 的循环逻辑、`FactsLog` 的追加逻辑、`FactSubmitter` 的通道发送逻辑。这些是“数据管道”。
- **禁止（策略）**：在反应器循环中判断“如果遇到某种特定 Command 则延迟 5 秒”或“如果是数学规则则优先处理”——这些必须是业务数据，要么在 `core_eval.json` 中，要么在业务规则 JSON 中，要么由 `IoSubscriber` 根据参数执行，但**绝不能**写在 `reactor.rs` 的 Rust 匹配分支中。

---

### 📋 强制执行机制

除了代码审查外，各层 crate 的 `build.rs` 会在编译时自动扫描源码，一旦发现上述禁止模式，构建将**直接失败**并提示违规详情（行号、违背条款）。若遇紧急调试需要，可临时设置环境变量 `EVORULE_SKIP_GATE=1` 跳过（仅限本地开发，严禁带入 CI/CD）。

**总结口诀**：

> **写 Rust 只写“怎么跑”（循环、路由、存日志），不写“跑什么”（阈值、模板、权限表）。凡是要根据业务变的值，统统放进 JSON。**
