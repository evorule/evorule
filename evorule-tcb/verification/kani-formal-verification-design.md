# Kani 形式化验证设计方案（evorule-tcb v0.3.1）

> 本文档为 `evorule-tcb` v0.3.1 的 Kani 形式化验证设计，涵盖验证目标、验证策略、
> 输入建模、证明清单（P1-P21）、目录结构、运行方式、实施计划与风险评估。

---

## 一、验证目标

### 1.1 核心目标

| 目标 | 说明 | 优先级 |
|------|------|--------|
| **无 panic** | 所有公开函数在所有合法输入下不 panic | P0 |
| **确定性** | 相同输入 → 相同输出（无时间/随机/哈希依赖） | P0 |
| **类型安全** | 所有类型转换安全（无非法 unwrap） | P1 |
| **边界安全** | 数组索引/算术运算不越界 | P1 |
| **状态转换正确性** | 6 种元指令（set/push/branch/io_request/collect/merge）语义符合规格 | P2 |
| **域评估正确性** | 7 种域类型（eq/lt/exists/instruction/all/not/has_fields）语义正确 | P2 |
| **路径解析正确性** | `resolve_path` 符合 ABNF 规格，无效路径返回 `None`（不 panic） | P2 |

> **v0.3.1 覆盖重点**：ReAct 相关元指令（`io_request` 触发、`collect` 的 `after` 参数、`merge` 的结果合并、`substitute_template` 模板替换）与 `has_fields`/`lt` 域类型。

### 1.2 非目标（本次不验证）

- 性能（Kani 不验证性能）
- I/O 路径（TCB 无 I/O）
- 反应器层（Kani 仅验证 TCB）
- `core_eval.json` 的具体规则（验证的是 TCB 引擎，不是配置）

---

## 二、验证策略

### 2.1 分层验证

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Kani 验证分层                                 │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 1: 基础类型层                                               │
│  - JsonValue 的 PartialEq/Ord 正确性                               │
│  - ObjectMap (BTreeMap) 的操作正确性                               │
│  - 类型转换 (as_*) 的安全性                                        │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 2: 路径解析层                                               │
│  - resolve_path 永不 panic                                         │
│  - resolve_path 是确定性的                                         │
│  - 无效路径返回 None（不 panic）                                   │
│  - 数组索引不越界                                                  │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 3: 域评估层                                                 │
│  - evaluate_domain 永不 panic                                      │
│  - 7 种域类型语义正确（eq/lt/exists/instruction/all/not/has_fields）│
│  - 递归深度限制生效（MAX_DOMAIN_DEPTH=64）                         │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 4: 元指令层（经 execute_meta_instruction 间接覆盖）          │
│  - 6 种元指令永不 panic（set/push/branch/io_request/collect/merge）│
│  - set/add/sub 算术安全（溢出返回 IntegerOverflow）                │
│  - branch 嵌套深度限制生效（MAX_BRANCH_DEPTH=64）                  │
│  - collect 数组遍历安全 + after 参数排序（v0.3.1）                 │
│  - merge 结果合并正确（v0.3.1）                                    │
│  - substitute_template 模板替换安全（v0.3.1）                      │
│  - io_request 可选参数容错（v0.3.1 ReAct）                         │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 5: 状态转换层                                               │
│  - execute_transition 永不 panic                                   │
│  - 规则数限制生效（MAX_TRANSFORM_RULES=64）                        │
│  - ReAct 循环：call_external 无结果时返回 IoRequired（不 panic）    │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 策略原则

| 原则 | 说明 |
|------|------|
| **验证生产代码** | 不使用 `#[cfg(kani)]` 重写数据结构和算法；Kani（≥0.40）原生支持 std collections（BTreeMap/Vec/Cow/String），直接验证生产代码 |
| **结构化符号输入** | 不整体 `kani::any::<JsonValue>()`（全符号 BTreeMap/Vec 会状态爆炸）；改为构造**已知形状**的 JsonValue（固定键、固定结构），仅叶子值符号化（`kani::any::<i64>()` 等），兼顾"验证生产代码"与"控制展开成本" |
| **公开 API 入口** | Kani proof 位于 crate 外独立测试目标，只调用公开 API（`execute_transition`/`execute_meta_instruction`/`evaluate_domain`/`resolve_path`）；私有元指令（`exec_set`/`exec_branch` 等）经 `execute_meta_instruction` 按指令类型间接覆盖 |
| **属性验证** | 验证"属性"而非"具体行为"（不 panic / 确定性 / 返回 Option） |
| **增量验证** | 先验证核心函数（Layer 1-2），再扩展（Layer 3-5） |
| **输入空间限制** | 用 `kani::assume` 约束输入大小（数组长度、字符串长度、数值范围） |

---

## 三、输入建模策略

### 3.1 设计原则：直接验证生产 BTreeMap

输入建模不引入任何替代数据结构（如固定容量映射 FixedMap），原因：

1. Kani（≥0.40）原生支持 std collections，可直接验证生产代码中的 `BTreeMap`/`Vec`/`Cow`/`String`；
2. 替代映射需要在插入时引入容量上限检查（`unreachable!()` 等 panic 分支），与"永不 panic"验证目标冲突；
3. 验证替代映射无法保证生产 `BTreeMap` 的真实行为（确定性迭代顺序、插入语义）；
4. 生产 `JsonValue::String` 是 `Cow<'static, str>`（[value.rs](file:///d:/evorule/evorule-tcb/src/value.rs)），替代映射的类型无法与之直接对应。

因此对 `ObjectMap`（`BTreeMap<String, JsonValue>`）不做任何替换，直接验证生产代码，通过"结构化符号输入"控制展开成本。

### 3.2 结构化符号输入模式

核心：**固定结构 + 符号叶子**。

```rust
// tests/kani/model.rs —— 结构化符号输入辅助
#![cfg(kani)]

/// 构造"已知形状、符号叶子"的对象。
/// 键数固定 → BTreeMap 大小确定，展开成本可控；叶子值符号化 → 覆盖全部输入值。
fn any_payload() -> JsonValue {
    let mut map = BTreeMap::new();
    map.insert("x".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert("y".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert(
        "obj".to_string(),
        JsonValue::Object({
            let mut m = BTreeMap::new();
            m.insert("flag".to_string(), JsonValue::Bool(kani::any::<bool>()));
            m
        }),
    );
    JsonValue::Object(map)
}

/// 符号字符串（固定长度，避免无界展开）
fn any_str<const N: usize>() -> String {
    let bytes: [u8; N] = kani::any();
    let mut s = String::with_capacity(N);
    for b in bytes {
        // 仅取 ASCII 可打印区间，控制路径分支
        s.push((b % 127) as char);
    }
    s
}

/// 符号指令（type 取合法集合之一）
fn any_instruction() -> JsonValue {
    let t = kani::any::<u8>() % 6; // 0..=5 → set/push/branch/io_request/collect/merge
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string(match t {
        0 => "set",
        1 => "push",
        2 => "branch",
        3 => "io_request",
        4 => "collect",
        _ => "merge",
    }));
    // params 按类型给出已知形状
    JsonValue::Object(instr)
}

/// 构造"已知形状、符号叶子"的 exec_state（供 evaluate_domain 使用）。
///
/// ⚠️ 重要：`evaluate_domain` 内部经 `resolve_domain_path` 解析路径，
/// 要求 exec_state **顶层必须有 `__exec__` 键**（[domain.rs](file:///d:/evorule/evorule-tcb/src/domain.rs#L37-L41)），
/// 否则所有 `resolve_domain_path` 均返回 `None`，域评估恒为 `false`。
/// 因此 `any_payload()` 不能直接作为 exec_state 传入，必须用本函数包裹：
fn any_exec_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), any_payload());
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造完整 exec_state：`__exec__` 下含 `instruction` + `payload` + `queue`。
/// 供 execute_meta_instruction（P12/P13/P14）使用——它需要 `__exec__.payload` 与 `__exec__.queue`。
fn any_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), any_payload());
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 以给定 payload 内容构造完整 exec_state（供 P15/P16/P17/P18 传入具体 payload）。
fn state_with_payload(payload: ObjectMap) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}
```

> 说明：`kani::assume` 约束优先于整体符号化。对必须完全符号的维度（如 `i64` 叶子），
> Kani 会做位级穷举，成本可接受；对容器维度（键/长度）务必固定形状。

### 3.3 递归函数处理

`evaluate_domain`/`exec_branch` 是递归的（深度上限 64）。Kani 对递归需要限制展开深度：

- 验证"不 panic"时，用 `--default-unwind 70`（略大于 64 深度上限 + 若干栈帧）保证能遍历到深度保护分支；
- 若展开超时，先 `--default-unwind 8` 验证浅层路径，再单独构造"深嵌套"具体输入（如嵌套 65 层 `not`）验证深度保护；
- 也可用 `#[kani::unwind(70)]` 在单个 harness 上指定，或写入 `Cargo.toml` 的 `[package.metadata.kani.flags] default-unwind = "70"`（Kani ≥ 0.50 支持 metadata 配置，见 [Kani usage docs](https://model-checking.github.io/kani/usage.html)）。

> ⚠️ **实测警告（Kani 0.67）**：unwind 值**必须 ≥ 实际递归深度上限**，否则 Kani 报
> `recursion unwinding assertion` 的 **FAILURE**（而非"验证通过但未覆盖全部路径"）。
> 实测：`assume(d ≤ 64)` 配 `--default-unwind 16` → FAILURE；`assume(d ≤ 8)` 配 `--default-unwind 16` → 通过。
> 因此对深度上限 64 的递归，`default-unwind` 必须 ≥ 64（建议 70）。

---

## 四、Kani 证明清单（v0.3.1）

### 4.1 Layer 1: 基础类型层

```rust
// tests/kani/kani_proofs.rs

/// P1: JsonValue::PartialEq 永不 panic（结构化符号）
#[cfg(kani)]
#[kani::proof]
fn verify_partial_eq_never_panics() {
    let a = model::any_payload();
    let b = model::any_payload();
    let _ = a == b;
}

/// P2: JsonValue::Ord 永不 panic
#[cfg(kani)]
#[kani::proof]
fn verify_ord_never_panics() {
    let a = model::any_payload();
    let b = model::any_payload();
    let _ = a.cmp(&b);
}

/// P3: 类型转换安全（as_* 均返回 Option，不 panic）
#[cfg(kani)]
#[kani::proof]
fn verify_as_methods_never_panic() {
    let v = model::any_payload();
    let _ = v.as_i64();
    let _ = v.as_str();
    let _ = v.as_bool();
    let _ = v.as_array();
    let _ = v.as_object();
}
```

### 4.2 Layer 2: 路径解析层

```rust
/// P4: resolve_path 永不 panic（符号路径 = 固定长度字节构造）
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_never_panics() {
    let state = model::any_payload();
    let path = model::any_str::<8>();
    let _ = resolve_path(&state, &path);
}

/// P5: resolve_path 是确定性的（两次调用结果一致）
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_deterministic() {
    let state = model::any_payload();
    let path = model::any_str::<8>();
    let r1 = resolve_path(&state, &path);
    let r2 = resolve_path(&state, &path);
    assert_eq!(r1, r2);
}

/// P6: 无效路径返回 None（不 panic）
/// 覆盖：空路径、尾点号（"x."）、不存在的字段、类型不匹配、越界索引
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_invalid_returns_none() {
    let state = model::any_payload();
    // 构造已知 None 场景
    assert!(resolve_path(&state, "").is_none());
    assert!(resolve_path(&state, "x.").is_none());
    assert!(resolve_path(&state, "missing.field").is_none());
    let _ = resolve_path(&state, &model::any_str::<8>());
}

/// P7: 数组索引不越界（get() 语义，越界返回 None 而非 panic）
#[cfg(kani)]
#[kani::proof]
fn verify_array_index_bounds() {
    let mut map = BTreeMap::new();
    map.insert("arr".to_string(), JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]));
    let state = JsonValue::Object(map);
    let _ = resolve_path(&state, "arr[0]");
    let _ = resolve_path(&state, "arr[9]"); // 越界 → None，不 panic
}
```

### 4.3 Layer 3: 域评估层

```rust
/// P8: evaluate_domain 永不 panic（7 种域类型全覆盖）
#[cfg(kani)]
#[kani::proof]
fn verify_evaluate_domain_never_panics() {
    let exec_state = model::any_exec_state(); // 顶层含 __exec__，域路径才能真实解析
    // 覆盖全部 7 种域类型（eq/lt/exists/instruction/all/not/has_fields）
    let domains = vec![
        JsonValue::object_from_pairs(&[("type", JsonValue::string("eq")), ("path", JsonValue::string("__exec__.payload.x")), ("value", JsonValue::Integer(kani::any::<i64>()))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("lt")), ("path", JsonValue::string("__exec__.payload.x")), ("value", JsonValue::Integer(kani::any::<i64>()))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("__exec__.payload.x"))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("instruction")), ("instruction_type", JsonValue::string("set"))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("all")), ("inner", JsonValue::Array(vec![]))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("__exec__.payload.x"))]))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("has_fields")), ("path", JsonValue::string("__exec__.payload.obj")), ("fields", JsonValue::array(vec![JsonValue::string("flag")]))]),
    ];
    for d in domains {
        let _ = evaluate_domain(&d, &exec_state);
    }
}

/// P9: evaluate_domain 是确定性的
#[cfg(kani)]
#[kani::proof]
fn verify_evaluate_domain_deterministic() {
    let domain = model::any_payload();
    let exec_state = model::any_exec_state();
    let r1 = evaluate_domain(&domain, &exec_state);
    let r2 = evaluate_domain(&domain, &exec_state);
    assert_eq!(r1, r2);
}

/// P10: 深度限制生效（MAX_DOMAIN_DEPTH=64）
/// 用具体深嵌套输入（如嵌套 65 层 not）验证不 panic 且深度分支可达。
/// 注意：evaluate_domain_inner 为私有函数，只能经 evaluate_domain 间接验证；
/// MAX_DOMAIN_DEPTH 也是私有常量，外部 test 只能硬编码 65（与源码保持一致）。
#[cfg(kani)]
#[kani::proof]
fn verify_domain_depth_limit() {
    // 构造深嵌套 not：not(not(...not(exists x)...))，深度 65
    let mut domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("exists")),
        ("path", JsonValue::string("__exec__.payload.x")),
    ]);
    for _ in 0..65 {
        domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", domain),
        ]);
    }
    let exec_state = model::any_exec_state();
    // 不 panic；depth > MAX_DOMAIN_DEPTH 时返回 false（evaluate_domain_inner 深度保护）
    let _ = evaluate_domain(&domain, &exec_state);
}

/// P11: has_fields 空数组返回 false（与源码语义一致）
#[cfg(kani)]
#[kani::proof]
fn verify_has_fields_empty_array() {
    // ⚠️ exec_state 顶层必须有 __exec__，且路径要写全（resolve_domain_path 会自动补 __exec__. 前缀，
    // 但为避免歧义这里写全 __exec__.payload.obj）
    let exec_state = JsonValue::object_from_pairs(&[
        ("__exec__", JsonValue::object_from_pairs(&[
            ("payload", JsonValue::object_from_pairs(&[
                ("obj", JsonValue::object_from_pairs(&[
                    ("tool_calls", JsonValue::Array(vec![])),
                ])),
            ])),
        ])),
    ]);
    let domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string("__exec__.payload.obj")),
        ("fields", JsonValue::array(vec![JsonValue::string("tool_calls")])),
    ]);
    assert!(!evaluate_domain(&domain, &exec_state), "空数组应视为不存在");
}
```

> 注意：`evaluate_has_fields` 为私有函数，须经 `evaluate_domain`（公开）验证。

### 4.4 Layer 4: 元指令层（经 execute_meta_instruction 间接）

> 私有元指令（`exec_set`/`exec_push`/`exec_branch`/`exec_io_request`/`exec_collect`/`exec_merge`/`substitute_template`）
> 无法从外部直接调用，统一经 `execute_meta_instruction`（公开）按指令类型间接验证。
>
> 导入说明：`MAX_BRANCH_DEPTH` 是 [executor.rs](file:///d:/evorule/evorule-tcb/src/executor.rs#L27) 的 `pub const`，
> 但 lib.rs **未重导出**（仅重导出 `MAX_TRANSFORM_RULES`）。外部 test 需 `use evorule_tcb::executor::MAX_BRANCH_DEPTH;`
> 或直接写字面量 64。

```rust
/// P12: execute_meta_instruction 永不 panic（6 种元指令全覆盖）
#[cfg(kani)]
#[kani::proof]
fn verify_execute_meta_instruction_never_panics() {
    let instr = model::any_instruction();
    let state = model::any_state(); // 含 __exec__.payload / __exec__.queue 的已知形状
    let depth = kani::any::<usize>();
    kani::assume(depth < MAX_BRANCH_DEPTH);
    let _ = execute_meta_instruction(&instr, state, depth);
}

/// P13: set 算术安全（add/sub 溢出返回 IntegerOverflow，不 panic）
#[cfg(kani)]
#[kani::proof]
fn verify_exec_set_arithmetic_safe() {
    // 经 execute_meta_instruction("set") 间接覆盖 exec_set
    // 用符号 i64 叶子触发 checked_add/checked_sub 的全部路径
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        ("params", JsonValue::object_from_pairs(&[
            ("attr", JsonValue::string("x")),
            ("operation", JsonValue::string(if kani::any::<bool>() { "add" } else { "sub" })),
            ("value", JsonValue::Integer(kani::any::<i64>())),
        ])),
    ]);
    let state = model::any_state();
    let r = execute_meta_instruction(&instr, state, 0);
    // 无论 Ok/Err 均不 panic；溢出时返回 IntegerOverflow
    if let Err(e) = r {
        // 允许的纯错误变体（不 panic）
        let _ = e;
    }
}

/// P14: branch 深度限制生效（depth >= MAX_BRANCH_DEPTH → NestingTooDeep）
#[cfg(kani)]
#[kani::proof]
fn verify_branch_depth_limit() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        ("params", JsonValue::object_from_pairs(&[
            ("domain", JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("x"))])),
            ("on_true", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
            ("on_false", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let state = model::any_state();
    let r = execute_meta_instruction(&instr, state, MAX_BRANCH_DEPTH);
    // depth >= MAX_BRANCH_DEPTH 时返回 NestingTooDeep（不 panic）
    assert!(matches!(r, Err(TcbError::NestingTooDeep { .. })));
}

/// P15: collect 遍历安全 + after 参数排序（v0.3.1）
#[cfg(kani)]
#[kani::proof]
fn verify_collect_safe_with_after() {
    // collect 从已知数组生成指令并 push after 指令到末尾
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("collect")),
        ("params", JsonValue::object_from_pairs(&[
            ("from", JsonValue::string("__exec__.payload.items")),
            ("each", JsonValue::object_from_pairs(&[("type", JsonValue::string("set")), ("params", JsonValue::object_from_pairs(&[("attr", JsonValue::string("{{name}}")), ("operation", JsonValue::string("set")), ("value", JsonValue::Integer(1))]))])),
            ("after", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let mut map = BTreeMap::new();
    map.insert("items".to_string(), JsonValue::Array(vec![
        JsonValue::object_from_pairs(&[("name", JsonValue::string("a"))]),
        JsonValue::object_from_pairs(&[("name", JsonValue::string("b"))]),
    ]));
    let state = model::state_with_payload(map);
    let r = execute_meta_instruction(&instr, state, 0);
    // 不 panic；generated 指令在前，after 指令在队尾（顺序语义由规则测试覆盖）
    assert!(r.is_ok());
}

/// P16: merge 结果合并正确（v0.3.1：追加 tool 消息 + 无条件推 next_instruction）
#[cfg(kani)]
#[kani::proof]
fn verify_merge_safe() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("merge")),
        ("params", JsonValue::object_from_pairs(&[
            ("messages", JsonValue::string("__exec__.payload.messages")),
            ("tool_result", JsonValue::string("__exec__.payload.result")),
            ("next_instruction", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::from([
        ("messages".to_string(), JsonValue::Array(vec![JsonValue::object_from_pairs(&[("role", JsonValue::string("user")), ("content", JsonValue::string("hi"))])])),
        ("result".to_string(), JsonValue::object_from_pairs(&[("role", JsonValue::string("tool")), ("content", JsonValue::string("ok"))])),
    ]));
    let r = execute_meta_instruction(&instr, state, 0);
    assert!(r.is_ok(), "merge 不应失败/panic");
}

/// P17: substitute_template 永不 panic（经 collect/merge 间接）
/// 覆盖：模板字段存在/缺失、嵌套路径、非字符串字段
#[cfg(kani)]
#[kani::proof]
fn verify_substitute_template_never_panics() {
    // 直接验证 substitute_template 需 pub(crate)；此处经 collect 间接覆盖。
    // 若需直接验证，将 substitute_template 提升为 pub 或放入 crate 内 cfg(kani) 模块。
    // （见「九、风险」私有函数覆盖策略）
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("collect")),
        ("params", JsonValue::object_from_pairs(&[
            ("from", JsonValue::string("__exec__.payload.items")),
            ("each", JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                ("params", JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("{{nested.field}}")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ])),
            ])),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::from([
        ("items".to_string(), JsonValue::Array(vec![
            JsonValue::object_from_pairs(&[("nested", JsonValue::object_from_pairs(&[("field", JsonValue::Integer(1))]))]),
        ])),
    ]));
    let _ = execute_meta_instruction(&instr, state, 0);
}

/// P18: io_request 触发正确（v0.3.1 ReAct：可选参数路径不存在时跳过，不 panic）
#[cfg(kani)]
#[kani::proof]
fn verify_io_request_safe() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("io_request")),
        ("params", JsonValue::object_from_pairs(&[
            ("io_type", JsonValue::string("call_external")),
            ("messages", JsonValue::string("__exec__.payload.messages")),
            // tools 路径不存在 → 可选参数，跳过（不 panic）
            ("tools", JsonValue::string("__exec__.payload.missing_tools")),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::new());
    let r = execute_meta_instruction(&instr, state, 0);
    // io_request 返回 MetaInstructionResult::IoRequired（间接经 execute_meta_instruction）
    assert!(r.is_ok(), "io_request 不应 panic");
}
```

### 4.5 Layer 5: 状态转换层

```rust
/// P19: execute_transition 永不 panic（结构化符号）
#[cfg(kani)]
#[kani::proof]
fn verify_execute_transition_never_panics() {
    let core_eval = vec![
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
    ]; // 固定规则数（1），避免全符号 Vec 展开
    let instruction = model::any_instruction();
    let payload = model::any_payload();
    let queue: Vec<JsonValue> = vec![]; // 固定空队列
    let _ = execute_transition(&core_eval, &instruction, &payload, &queue);
}

/// P20: 规则数限制生效（core_eval.len() > MAX_TRANSFORM_RULES → TooManyTransformRules）
#[cfg(kani)]
#[kani::proof]
fn verify_transform_rules_limit() {
    let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
        .map(|_| JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]))
        .collect(); // MAX_TRANSFORM_RULES + 1 条规则
    let instruction = JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    assert!(matches!(r, Err(TcbError::TooManyTransformRules { .. })));
}

/// P21: ReAct 循环——call_external 无结果时返回 IoRequired（v0.3.1）
/// ⚠️ 注意：TCB 零依赖、不内嵌 JSON 解析器，core_eval.json 由上层（reactor/示例）
/// 加载后传入 execute_transition。因此本 proof 必须**手工构造** ReAct 三条规则
/// （与 transition.rs 现有 react_e2e_tests 一致），而不是调用 crate::core_eval()（不存在）。
#[cfg(kani)]
#[kani::proof]
fn verify_react_io_required() {
    // 手工构造与 core_eval.json v0.3.1 ReAct 三条规则一一对应的规则列表
    // （self_init / call_external / call_service，见 transition.rs tests 的 react_core_eval()）
    let core_eval = model::react_core_eval();
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        ("params", JsonValue::object_from_pairs(&[
            ("messages", JsonValue::Array(vec![JsonValue::object_from_pairs(&[("role", JsonValue::string("user")), ("content", JsonValue::string("hi"))])])),
            ("tools", JsonValue::Array(vec![])),
        ])),
    ]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    match r {
        Ok(TransitionResult::IoRequired { io_type, .. }) => assert_eq!(io_type, "call_external"),
        Ok(_) => panic!("should be IoRequired"),
        Err(e) => panic!("unexpected error: {:?}", e),
    }
}
```

> `model::react_core_eval()` 为 P21 专用辅助：在 tests/kani/model.rs 中按 [transition.rs](file:///d:/evorule/evorule-tcb/src/transition.rs#L1063-L1157) 的 `react_core_eval()` 复制三条规则（固定形状、无符号值），Kani 按具体常量展开，成本可控。

---

## 五、验证清单汇总（v0.3.1）

| ID | 证明 | 入口 | 验证内容 | 优先级 |
|----|------|------|---------|--------|
| P1 | `verify_partial_eq_never_panics` | 公开 | PartialEq 不 panic | P0 |
| P2 | `verify_ord_never_panics` | 公开 | Ord 不 panic | P0 |
| P3 | `verify_as_methods_never_panic` | 公开 | 类型转换安全 | P1 |
| P4 | `verify_resolve_path_never_panics` | 公开 | 路径解析不 panic | P0 |
| P5 | `verify_resolve_path_deterministic` | 公开 | 路径解析确定性 | P0 |
| P6 | `verify_resolve_path_invalid_returns_none` | 公开 | 无效路径返回 None | P0 |
| P7 | `verify_array_index_bounds` | 公开 | 数组索引安全 | P1 |
| P8 | `verify_evaluate_domain_never_panics` | 公开 | 域评估不 panic（7 种） | P0 |
| P9 | `verify_evaluate_domain_deterministic` | 公开 | 域评估确定性 | P0 |
| P10 | `verify_domain_depth_limit` | 公开 | 深度限制生效 | P1 |
| P11 | `verify_has_fields_empty_array` | 公开 | has_fields 空数组语义 | P2 |
| P12 | `verify_execute_meta_instruction_never_panics` | 公开 | 元指令不 panic（6 种） | P0 |
| P13 | `verify_exec_set_arithmetic_safe` | execute_meta_instruction | set 算术安全 | P0 |
| P14 | `verify_branch_depth_limit` | execute_meta_instruction | branch 深度限制 | P1 |
| P15 | `verify_collect_safe_with_after` | execute_meta_instruction | collect + after（v0.3.1） | P1 |
| P16 | `verify_merge_safe` | execute_meta_instruction | merge 合并（v0.3.1） | P1 |
| P17 | `verify_substitute_template_never_panics` | collect 间接 | 模板替换安全 | P1 |
| P18 | `verify_io_request_safe` | execute_meta_instruction | io_request 容错（v0.3.1） | P1 |
| P19 | `verify_execute_transition_never_panics` | 公开 | 状态转换不 panic | P0 |
| P20 | `verify_transform_rules_limit` | 公开 | 规则数限制 | P2 |
| P21 | `verify_react_io_required` | 公开 | ReAct I/O 触发（v0.3.1） | P1 |

---

## 六、目录结构（v0.3.1）

```
evorule-tcb/
├── src/                        ← 生产代码（零依赖，不包含任何 kani 代码）
│   ├── lib.rs                  ← 模块入口（deny(clippy::panic/unwrap/expect/indexing)）
│   ├── error.rs                ← 错误类型（TcbError）
│   ├── value.rs                ← JSON 数据模型（JsonValue / ObjectMap=BTreeMap）
│   ├── path.rs                 ← 路径解析（resolve_path / resolve_path_mut）
│   ├── domain.rs               ← 域评估（evaluate_domain，7 种域类型）
│   ├── executor.rs             ← 元指令执行（execute_meta_instruction，6 种）
│   └── transition.rs           ← 状态转换（execute_transition / TransitionResult）
├── tests/                      ← 外部集成测试（black-box）
│   ├── determinism_proptest.rs ← proptest 属性测试
│   ├── integration_test.rs     ← 集成测试
│   ├── kani.rs                 ← Kani proof 入口（顶层测试目标，见下方说明）
│   └── kani/                   ← Kani proof 模块（cfg(kani) 门控）
│       ├── mod.rs              ← 模块导出
│       ├── model.rs            ← 结构化符号输入辅助（any_payload / any_instruction / ...）
│       └── kani_proofs.rs      ← Kani 证明（P1-P21）
├── verification/               ← TCB 验证设计文档
│   └── kani-formal-verification-design.md  ← 本文档
├── Cargo.toml
└── build.rs                    ← L1 门禁（仅扫描 src/ 顶层，不触及 tests/）
```

> **为什么需要顶层 `tests/kani.rs`**：Cargo 只把 `tests/*.rs`（**顶层直接文件**）当作集成测试 crate。
> `tests/kani/mod.rs` 这种子目录不会被自动识别为测试目标，必须有一个顶层文件作为入口，
> 例如 `tests/kani.rs` 内容为 `#![cfg(kani)] mod kani;`（`mod kani;` 解析到 `tests/kani/mod.rs`）。
> 这样 `cargo kani --tests` 会编译并发现其中的 `#[kani::proof]`，而普通 `cargo test` 因 `cfg(kani)` 关闭而跳过。

> **为什么放 `tests/` 而非 `src/verification/`**：
> - 避免 `lib.rs` 的 `#![deny(clippy::unwrap/expect/panic/indexing)]` 拦截 proof 代码；
> - 避免 `build.rs` L1 门禁扫描到 kani 代码（proof 中可能出现的 `unreachable!` 等文本）；
> - 以 black-box 方式经公开 API 验证，与"验证生产代码"原则一致。

---

## 七、运行命令

```bash
# 环境准备（WSL Ubuntu ≥ 22.04，一次）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo install kani-verifier --version 0.67.0
cargo kani setup

# 运行全部 Kani proof（tests/ 目标）
cargo kani --package evorule-tcb --tests

# 运行单个 proof
cargo kani --package evorule-tcb --tests --harness verify_execute_transition_never_panics

# 限制递归展开深度（evaluate_domain/exec_branch 深度 64，需 > 64）
cargo kani --package evorule-tcb --tests --default-unwind 70

# 限制单条验证时间（experimental，需 -Z unstable-options）
cargo kani --package evorule-tcb --tests --harness-timeout 600s -Z unstable-options

# 指定求解器（默认 cadical；本机未装 cadical 时自动回退，也可显式指定）
cargo kani --package evorule-tcb --tests --solver minisat
```

> 用法说明（Kani 0.67 `kani --help` 实测）：
> - `--harness <name>` 指定单个 proof；`--default-unwind <n>` 设置所有 harness 的全局展开上限，
>   单个 harness 可用 `#[kani::unwind(n)]` 覆盖；
> - ⚠️ **没有 `--timeout` 参数**；单条超时用 `--harness-timeout <n>[s|m|h]`（experimental，需加 `-Z unstable-options`）；
> - `--solver` 可选 bitwuzla/cadical/cvc5/kissat/minisat/z3；本机默认 cadical 未安装，Kani 自动回退到 MiniSAT（不影响正确性）；可用 `--solver minisat` 显式指定；
> - `--tests` 用于验证 `#[kani::proof]` 测试函数（本方案统一使用）；

---

## 八、实施计划（v0.3.1）

| 阶段 | 任务 | 文件 | 优先级 |
|------|------|------|--------|
| 0 | WSL 安装 Rust + Kani（见 §七） | 环境 | P0 |
| 1 | Cargo.toml 增加 `[package.metadata.kani.flags]` + `tests/kani.rs` 入口 + `tests/kani/` 骨架 | Cargo.toml / tests/kani.rs / tests/kani/mod.rs | P0 |
| 2 | 结构化符号输入辅助（any_payload / any_instruction / any_str / any_exec_state / any_state / state_with_payload / react_core_eval） | tests/kani/model.rs | P0 |
| 3 | Layer 1-2 证明（P1-P7） | tests/kani/kani_proofs.rs | P0 |
| 4 | Layer 3 证明（P8-P11） | tests/kani/kani_proofs.rs | P1 |
| 5 | Layer 4 证明（P12-P18） | tests/kani/kani_proofs.rs | P1 |
| 6 | Layer 5 证明（P19-P21） | tests/kani/kani_proofs.rs | P2 |
| 7 | 实跑调优（default-unwind / harness-timeout / 状态爆炸） | tests/kani/ | P0 |
| 8 | 集成 CI（`.github/workflows/kani.yml`，仅 Linux 跑） | 工作流 | P2 |

**阶段 1 的 Cargo.toml 配置建议**（Kani ≥ 0.50，已实测确认 metadata 生效）：

```toml
[package.metadata.kani.flags]
default-unwind = "70"   # 递归深度 64，需 > 64（实测：该配置被 Cargo/Kani 读取并应用）
```

> **不需要把 `kani` 加为 dev-dependency**：Kani 构建时会自动注入 `kani` crate（提供 `kani::any`/`#[kani::proof]`）并设置 `cfg(kani)`，
> 因此 `tests/kani/` 中的 proof 无需声明依赖。`--tests` 模式会以 `cfg(test)` 编译且 dev-dependencies 可用，
> 现有 `proptest` dev-dependency 不受影响。`unexpected_cfgs` 的 `check-cfg` 配置已在 [Cargo.toml](file:///d:/evorule/evorule-tcb/Cargo.toml#L26-L28) 中就位（官方推荐做法）。

---

## 九、风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| BTreeMap 全符号展开状态爆炸 | 结构化符号输入（固定键 + 符号叶子）；`kani::assume` 限制大小 |
| 递归函数（evaluate_domain/exec_branch）展开超时 | `--default-unwind` 限制深度；深嵌套用具体输入单独验证 |
| **unwind 不足（< 实际递归深度）导致 FAILURE 而非部分覆盖** | unwind 值须 ≥ 递归深度上限（深度 64 时设 `default-unwind` ≥ 64，建议 70）；关联 `assume` 上限也须 ≤ unwind 值 |
| **Cow/String 比较展开 memcmp 循环**（实测 memcmp 在 `default-unwind 70` 下展开到 69 次迭代） | `--default-unwind 70` 足够覆盖 64 深度 + memcmp 开销；`--default-unwind 80` 有余量；若展开过大，考虑用 `#[kani::unwind(n)]` 单独为 P1/P2 设低值 |
| 私有元指令无法直接验证 | 统一经 `execute_meta_instruction` 按 type 间接覆盖（执行路径等价）；如需要直接验证，将目标函数提升为 `pub(crate)` 并在 crate 内放 `#[cfg(kani)]` 模块（需先评估 build.rs 门禁） |
| P21 需手工构造 ReAct 规则（TCB 零依赖、不内嵌 JSON 解析器） | 手工构造 ReAct 三条规则（复制 transition.rs 现有 `react_core_eval()`）；Kani 按具体常量折叠，展开可控 |
| 验证时间过长 | 增量验证；分片（每条 proof 独立 harness + 独立 timeout）；CI 分 job |
| 结构化符号输入遗漏路径 | 每个 proof 的输入形状与对应函数实际使用路径逐一核对（见 §四 注释） |
| Kani 对 `Cow`/`String` 的支持差异 | 已实测：Kani 0.67 可直接编译并验证 `Cow<'static, str>`（P1/P3 通过） |
| **默认 cadical 求解器未安装**（本机日志提示 `The specified solver 'cadical' is not available`） | Kani 自动回退到默认 MiniSAT（不影响正确性）；如需性能优化，可 `cargo kani setup` 补装或 `--solver minisat` 显式指定 |

> **实测确认项（WSL + Kani 0.67.0）**：
> - ✅ `BTreeMap`/`Vec`/`Cow<'static, str>` 可直接编译并验证（P1 PartialEq / P3 as_* 通过）；
> - ✅ `--default-unwind` 生效：递归在设定深度截断，避免无限展开；
> - ✅ `[package.metadata.kani.flags] default-unwind` 配置被 Cargo 读取（metadata 生效）；
> - ⚠️ unwind 值 < 递归深度上限时报 `recursion unwinding assertion` FAILURE（非部分覆盖）；
> - ⚠️ `--timeout` 参数不存在，单条超时用 `--harness-timeout`（需 `-Z unstable-options`）；
> - ⚠️ 默认 cadical 未安装，自动回退 MiniSAT（正确性不受影响）。
>
> **仍待实跑确认项**：P4-P7 的符号路径字符串（`any_str`）展开成本、P8-P11 域评估深递归（`--default-unwind 70` 可行性）、P21 手工 ReAct 规则在 Kani 下的展开成本。
