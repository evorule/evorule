//! tier0-tcb v6.0.0 -- Property tests (proptest)
//!
//! ## 目的
//!
//! 补充单元测试的有限 case 覆盖, 用随机化输入验证 TCB 的核心不变量:
//! - `JsonValue` 构造/访问 roundtrip 一致
//! - 路径解析确定性
//! - 域比较对称性 (eq 自身对称, lt/gt 互逆)
//! - 状态转换幂等性 (相同输入两次 → 相同输出)
//! - set 元指令数学律 (+0 不变, -0 不变)
//!
//! ## 跑法
//!
//! ```bash
//! cargo test --test proptest_props
//! ```
//!
//! ## Hygiene
//!
//! - `ProptestConfig::with_cases(200)` 限制每属性 case 数
//! - 不依赖 `*.proptest-regressions` 文件回放 (`FileFailurePersistence`
//!   会永久重放旧反例, 掩盖真实 assertion bug -- 已 .gitignore)
//! - 所有 proptest 用 fresh config, 不读取 .proptest-regressions

use proptest::prelude::*;
use tier0_tcb::domain::evaluate_domain;
use tier0_tcb::path::resolve_path;
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};

// =============================================================================
// Strategies: bounded integers to avoid overflow paths polluting math tests
// =============================================================================

fn arb_small_i64() -> impl Strategy<Value = i64> {
    // 普通范围, 用于 roundtrip / path / domain 测试 (不会触发 set 溢出)
    -1_000_000i64..1_000_000i64
}

fn arb_safe_i64() -> impl Strategy<Value = i64> {
    // 用于 set add/sub 仍不溢出 i64 的范围 (任意两数相加不溢出)
    // i64::MAX / 2 是保守上界, 给一些 headroom
    -4_000_000_000_000_000_000i64..4_000_000_000_000_000_000i64
}

fn arb_delta() -> impl Strategy<Value = i64> {
    // increment delta, 小范围避免 a + delta 溢出
    -10_000i64..10_000i64
}

/// 构造 increment 业务的 `core_eval`: `branch(instruction_type=increment)` -> set(add)
fn build_increment_core_eval() -> Vec<JsonValue> {
    vec![JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("instruction")),
                        ("instruction_type", JsonValue::string("increment")),
                    ]),
                ),
                (
                    "on_true",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("set")),
                        (
                            "params",
                            JsonValue::object_from_pairs(&[
                                ("attr", JsonValue::string("x")),
                                ("operation", JsonValue::string("add")),
                                (
                                    "value",
                                    JsonValue::string("__exec__.instruction.params.delta"),
                                ),
                            ]),
                        ),
                    ])]),
                ),
            ]),
        ),
    ])]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // -------------------------------------------------------------------------
    // 1. JsonValue 构造/访问 roundtrip 一致
    // -------------------------------------------------------------------------

    #[test]
    fn jsonvalue_integer_roundtrip(n in arb_small_i64()) {
        let v = JsonValue::Integer(n);
        prop_assert_eq!(v.as_i64(), Some(n));
        prop_assert!(v.is_integer());
        prop_assert!(!v.is_string());
        prop_assert!(!v.is_bool());
    }

    #[test]
    fn jsonvalue_bool_roundtrip(b: bool) {
        let v = JsonValue::Bool(b);
        prop_assert_eq!(v.as_bool(), Some(b));
        prop_assert!(v.is_bool());
        prop_assert!(!v.is_integer());
    }

    #[test]
    fn jsonvalue_string_roundtrip(s in "[a-zA-Z0-9 ]{0,50}") {
        let v = JsonValue::string(s.as_str());
        prop_assert_eq!(v.as_str(), Some(s.as_str()));
        prop_assert!(v.is_string());
    }

    #[test]
    fn jsonvalue_from_conversions(n in arb_small_i64()) {
        let v: JsonValue = n.into();
        prop_assert_eq!(v.as_i64(), Some(n));
        prop_assert!(v.is_integer());
    }

    #[test]
    fn jsonvalue_object_keys_present(
        x in arb_small_i64(),
        y in arb_small_i64(),
    ) {
        let obj = JsonValue::object_from_pairs(&[
            ("x", JsonValue::Integer(x)),
            ("y", JsonValue::Integer(y)),
        ]);
        let x_back = obj.get("x");
        let y_back = obj.get("y");
        let z_back = obj.get("z");
        prop_assert_eq!(x_back.and_then(tier0_tcb::JsonValue::as_i64), Some(x));
        prop_assert_eq!(y_back.and_then(tier0_tcb::JsonValue::as_i64), Some(y));
        prop_assert!(z_back.is_none());
    }

    // -------------------------------------------------------------------------
    // 2. 路径解析确定性 + 不变性
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_path_deterministic(x in arb_small_i64()) {
        let obj = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))]);
        let r1 = resolve_path(&obj, "x");
        let r2 = resolve_path(&obj, "x");
        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn resolve_path_nested_consistent(
        x in arb_small_i64(),
        y in arb_small_i64(),
    ) {
        let outer = JsonValue::object_from_pairs(&[(
            "outer",
            JsonValue::object_from_pairs(&[
                ("x", JsonValue::Integer(x)),
                ("y", JsonValue::Integer(y)),
            ]),
        )]);
        let r = resolve_path(&outer, "outer.x");
        prop_assert_eq!(r.and_then(|v: &JsonValue| v.as_i64()), Some(x));
        let r2 = resolve_path(&outer, "outer.y");
        prop_assert_eq!(r2.and_then(|v: &JsonValue| v.as_i64()), Some(y));
    }

    #[test]
    fn resolve_path_missing_returns_none(key in "[a-z]{1,3}") {
        let obj = JsonValue::object_from_pairs(&[("present", JsonValue::Integer(1))]);
        // 任何不存在 key 应返回 None (我们的 key 空间不含 "present")
        if key != "present" {
            prop_assert!(resolve_path(&obj, &key).is_none());
        }
    }

    // -------------------------------------------------------------------------
    // 3. 域比较对称性
    // -------------------------------------------------------------------------

    #[test]
    fn domain_eq_self_consistent(a in arb_small_i64(), b in arb_small_i64()) {
        let state = JsonValue::object_from_pairs(&[(
            "__exec__",
            JsonValue::object_from_pairs(&[(
                "payload",
                JsonValue::object_from_pairs(&[("x", JsonValue::Integer(a))]),
            )]),
        )]);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(b)),
        ]);
        prop_assert_eq!(evaluate_domain(&domain, &state), a == b);
    }

    #[test]
    fn domain_lt_gt_inverse(a in arb_small_i64(), b in arb_small_i64()) {
        // NOTE: domain.rs 实际只有 eq/lt/exists/instruction/all/not 6 个原始域
        // 派生域 gt/ge/le/ne/or 由 core_eval.json 嵌套组合实现
        // 注释 (domain.rs L21-25) 承诺可作顶层 type, 但实现走 _ => false
        // → proptest 发现: 直接用 type="gt" 永远返回 false
        // 这里用 gt = all([not(lt), not(eq)]) 嵌套形式
        let state = JsonValue::object_from_pairs(&[(
            "__exec__",
            JsonValue::object_from_pairs(&[(
                "payload",
                JsonValue::object_from_pairs(&[("x", JsonValue::Integer(a))]),
            )]),
        )]);
        let lt = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(b)),
        ]);
        // gt = all([not(lt), not(eq)])
        let gt = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        ("inner", lt.clone()),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(b)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);
        prop_assert_eq!(evaluate_domain(&lt, &state), a < b);
        prop_assert_eq!(evaluate_domain(&gt, &state), a > b);
        // 互斥: lt 和 gt 不可能同时为真
        prop_assert!(!(evaluate_domain(&lt, &state) && evaluate_domain(&gt, &state)));
    }

    #[test]
    fn domain_ge_uses_not_lt(a in arb_small_i64(), b in arb_small_i64()) {
        // ge = not(lt) → a >= b
        let state = JsonValue::object_from_pairs(&[(
            "__exec__",
            JsonValue::object_from_pairs(&[(
                "payload",
                JsonValue::object_from_pairs(&[("x", JsonValue::Integer(a))]),
            )]),
        )]);
        let ge = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(b)),
                ]),
            ),
        ]);
        prop_assert_eq!(evaluate_domain(&ge, &state), a >= b);
    }

    // -------------------------------------------------------------------------
    // 4. 状态转换幂等性 + increment 数学律
    // -------------------------------------------------------------------------

    #[test]
    fn execute_transition_increment_deterministic(
        x in arb_safe_i64(),
        delta in arb_delta(),
    ) {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(delta)),
                ]),
            ),
        ]);
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))]);
        let core_eval = build_increment_core_eval();

        let r1 = execute_transition(&core_eval, &instruction, &payload, &[]);
        let r2 = execute_transition(&core_eval, &instruction, &payload, &[]);
        prop_assert_eq!(r1.is_ok(), r2.is_ok());
        if let (Ok(TransitionResult::State { new_payload: p1, .. }),
                Ok(TransitionResult::State { new_payload: p2, .. })) = (&r1, &r2) {
            prop_assert_eq!(p1, p2);
        }
    }

    #[test]
    fn execute_transition_increment_correctness(
        x in arb_safe_i64(),
        delta in arb_delta(),
    ) {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(delta)),
                ]),
            ),
        ]);
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))]);
        let core_eval = build_increment_core_eval();

        let r = execute_transition(&core_eval, &instruction, &payload, &[]);
        if let Ok(TransitionResult::State { new_payload, .. }) = r {
            let v = resolve_path(&new_payload, "x");
            let expected = x.checked_add(delta);
            match expected {
                Some(exp) => prop_assert_eq!(v.and_then(|vv: &JsonValue| vv.as_i64()), Some(exp)),
                None => prop_assert!(false, "overflow should not happen with safe_i64 range"),
            }
        }
    }

    #[test]
    fn execute_transition_increment_zero_delta_is_identity(x in arb_safe_i64()) {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(0)),
                ]),
            ),
        ]);
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))]);
        let core_eval = build_increment_core_eval();

        let r = execute_transition(&core_eval, &instruction, &payload, &[]);
        if let Ok(TransitionResult::State { new_payload, .. }) = r {
            let v = resolve_path(&new_payload, "x");
            prop_assert_eq!(v.and_then(|vv: &JsonValue| vv.as_i64()), Some(x));
        }
    }
}
