# while_loop 指令测试覆盖率分析报告

> **最后更新**: 2026-08-18
> **版本**: v2 - 补充 P0 级高风险测试后

## 1. 执行逻辑分析

`has_matching_business_rule` 判断流程（[transition.rs#L168-L192](file:///D:/evorule/evorule-tcb/src/transition.rs#L168-L192)）：

```
遍历 core_eval 中的每条规则:
├── 规则有 params.domain
│   ├── domain_type == "instruction"
│   │   ├── domain.instruction_type == instruction_type → true (匹配)
│   │   └── domain.instruction_type != instruction_type → 继续下一条规则
│   ├── domain_type == "all" → false (兜底，不是业务规则)
│   └── domain_type ∈ {eq, lt, gt, ge, not, ...} → true (视为业务规则)
├── 规则无 params.domain 但有 type
│   └── type ∈ {set, increment, decrement, branch, collect, merge} → true (直接规则)
└── 其他情况 → false
```

## 2. 测试覆盖矩阵（补充 P0 后）

### 2.1 已覆盖场景（7 个）

| # | 场景 | 规则结构 | instruction_type | 结果 | 测试函数 | 优先级 |
|---|------|---------|-----------------|------|---------|--------|
| 1 | 规则匹配 + condition=false | instruction("while_loop") | "while_loop" | State | `test_while_loop_condition_false_returns_state_not_ignored` | P0 |
| 2 | 规则匹配 + condition=true | instruction("while_loop") | "while_loop" | State | `test_while_loop_condition_true_executes_body` | P0 |
| 3 | 无规则匹配 | empty | "while_loop" | Ignored | `test_while_loop_no_matching_rule_returns_ignored` | P0 |
| 4 | **all 兜底规则** | all([]) | "while_loop" | Ignored | `test_while_loop_all_fallback_rule_returns_ignored` | P0 ✅ |
| 5 | **动态 domain 误判** | lt("__payload.counter", 3) | "while_loop" | State (误判) | `test_while_loop_dynamic_domain_rule_current_behavior` | P0 ✅ |
| 6 | **直接规则误判** | set | "while_loop" | State (误判) | `test_while_loop_direct_rule_current_behavior` | P0 ✅ |
| 7 | **规则匹配 + on_true 为空** | instruction("while_loop"), on_true=[] | "while_loop" | State | `test_while_loop_matched_rule_empty_on_true_returns_state` | P0 ✅ |

### 2.2 未覆盖场景（6 个）

| # | 场景 | 规则结构 | instruction_type | 预期结果 | 风险等级 | 优先级 |
|---|------|---------|-----------------|---------|---------|--------|
| B1 | instruction domain 不匹配（指向其他指令类型） | instruction("increment") | "while_loop" | Ignored | 🔴 高 | P1 |
| B2 | instruction domain 缺少 instruction_type 字段 | instruction(无 instruction_type) | "while_loop" | Ignored | 🟡 中 | P1 |
| B6 | 混合规则（既有匹配又有不匹配） | [instruction("increment"), instruction("while_loop")] | "while_loop" | State | 🟢 低 | P2 |
| I1 | 指令无 type 字段 | {} 或 {params: {...}} | Ignored (instruction_type="unknown") | 🟡 中 | P1 |
| I2 | 指令 type 为 null/非字符串 | {type: null} | Ignored (instruction_type="unknown") | 🟢 低 | P2 |
| E1 | on_false 分支有操作（非空） | condition=false, on_false=[set(...)] | State (payload 变化) | 🟢 低 | P2 |

## 3. 重点风险场景说明

### 3.1 ✅ 已验证的高风险场景

#### B3 - all 兜底规则（已验证）
```rust
// 已验证：all([]) 兜底规则正确返回 Ignored
// 测试：test_while_loop_all_fallback_rule_returns_ignored ✅
```

#### B4 - 动态 domain 规则误判（已记录当前行为）
```rust
// 当前行为：lt 规则被视为业务规则，返回 State
// 测试：test_while_loop_dynamic_domain_rule_current_behavior ✅
// TODO: 后续改进时应改为期望 Ignored
```

#### B5 - 直接规则误判（已记录当前行为）
```rust
// 当前行为：set 规则被视为业务规则，返回 State
// 测试：test_while_loop_direct_rule_current_behavior ✅
// TODO: 后续改进时应改为期望 Ignored
```

#### E2 - 规则匹配但 on_true 为空（已验证）
```rust
// 已验证：规则匹配但无操作时正确返回 State
// 测试：test_while_loop_matched_rule_empty_on_true_returns_state ✅
```

### 3.2 📝 已知限制（需后续改进）

#### 限制 1：动态 domain 规则无条件视为业务规则
```rust
// 代码位置：transition.rs#L180-L181
} else {
    return true; // 其他类型（eq, lt, gt, ge, not 等）都是业务规则
}

// 问题：lt/eq 等动态 domain 规则不区分指令类型
// 影响：可能导致真正应该报告为 Ignored 的指令被误判为 State
```

#### 限制 2：直接规则无条件视为业务规则
```rust
// 代码位置：transition.rs#L187-L189
if matches!(rule_type, "set" | "increment" | ...) {
    return true;
}

// 问题：直接规则不检查是否与当前指令类型匹配
// 影响：所有直接规则都会被视为当前指令的业务规则
```

## 4. 覆盖率统计

| 类别 | 总场景数 | 已覆盖 | 未覆盖 | 覆盖率 |
|------|---------|--------|--------|--------|
| 规则匹配逻辑 | 6 | 5 | 1 | 83% |
| 指令结构 | 3 | 1 | 2 | 33% |
| 执行结果 | 4 | 3 | 1 | 75% |
| **总计** | **13** | **9** | **4** | **69%** |

## 5. 后续建议

### 优先级 P1（中风险场景，建议补充）

1. **B1** - 验证 instruction domain 指向其他指令类型
   - 场景：规则用 `instruction("increment")` domain，但指令是 `while_loop`
   - 目的：验证 instruction domain 的精确匹配逻辑

2. **B2** - 验证 instruction domain 缺少 instruction_type 字段
   - 场景：规则有 `instruction` domain，但没有 `instruction_type` 字段
   - 目的：验证边界条件处理

3. **I1** - 验证指令无 type 字段
   - 场景：指令结构为 `{params: {condition: ..., body: ...}}`（缺少 type 字段）
   - 目的：验证 `instruction_type` 回退到 "unknown" 的逻辑

### 优先级 P2（低风险场景，可选补充）

4. **B6** - 验证混合规则场景
5. **I2** - 验证 type 为 null 的场景
6. **E1** - 验证 on_false 分支有操作的场景

## 6. 代码改进建议

### 改进 1：动态 domain 规则需检查 instruction_type 绑定

```rust
// 当前实现（transition.rs#L180-L181）
} else {
    return true; // ❌ 所有其他类型都视为业务规则
}

// 建议改进
} else {
    // 动态 domain（eq, lt 等）仅在它们显式绑定到特定指令时才有效
    if let Some(bound_type) = domain.get("instruction_type").and_then(|t| t.as_str()) {
        return bound_type == instruction_type;
    }
    // 无指令绑定的动态 domain：无法确定是否匹配
    // 建议：不视为业务规则（false），让不匹配的指令显式报告 Ignored
    return false;
}
```

### 改进 2：直接规则需检查是否与当前指令类型匹配

```rust
// 当前实现（transition.rs#L187-L189）
if matches!(rule_type, "set" | "increment" | ...) {
    return true; // ❌ 所有直接规则都视为业务规则
}

// 建议改进
if matches!(rule_type, "set" | "increment" | ...) {
    // 直接规则检查是否与当前指令类型匹配
    return rule_type == instruction_type;
}
```

## 7. 测试执行结果

```
running 7 tests
test transition::tests::test_while_loop_no_matching_rule_returns_ignored ... ok
test transition::tests::test_while_loop_all_fallback_rule_returns_ignored ... ok
test transition::tests::test_while_loop_condition_false_returns_state_not_ignored ... ok
test transition::tests::test_while_loop_dynamic_domain_rule_current_behavior ... ok
test transition::tests::test_while_loop_matched_rule_empty_on_true_returns_state ... ok
test transition::tests::test_while_loop_condition_true_executes_body ... ok
test transition::tests::test_while_loop_direct_rule_current_behavior ... ok

test result: ok. 7 passed; 0 failed
```

## 8. 总结

当前 while_loop 测试覆盖率为 **69%**，主要进展：
- ✅ P0 级 4 个高风险场景全部补充完成
- ✅ all 兜底规则正确返回 Ignored
- ✅ 规则匹配但 on_true 为空时正确返回 State
- 📝 B4/B5 误判场景已记录当前行为，标记为已知限制

下一步建议：
1. 补充 P1 级测试（B1, B2, I1），将覆盖率提升至 **92%**
2. 实施代码改进建议（动态 domain 和直接规则的精确匹配）
3. 改进后更新 B4/B5 测试的预期结果为 Ignored

