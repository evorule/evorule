// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
#![allow(clippy::panic, clippy::expect_used)]
//! 复杂分支逻辑测试 - 电商订单处理场景
//!
//! 测试场景：
//! - 如果指令类型是 `process_order`：
//!   - 如果存在 `is_vip` 字段：
//!     - 设置 `discount = 10`（表示 9 折）
//!     - 触发 I/O：VIP 折扣通知
//!   - 否则：
//!     - 设置 `discount = 0`
//!   - 如果存在 `inventory_sufficient`：
//!     - 扣减库存（sub）
//!     - 触发 I/O：订单确认
//!   - 否则：
//!     - 触发 I/O：库存预警

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use tier0_tcb::{execute_transition, JsonValue, TransitionResult};

/// 构造电商订单处理的 core_eval 规则
#[allow(clippy::too_many_lines)]
fn order_processing_core_eval() -> Vec<JsonValue> {
    vec![
        // 规则 1: 匹配 process_order 指令
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("process_order")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![
                            // 分支 1: 检查是否 VIP 客户
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("branch")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        (
                                            "domain",
                                            JsonValue::object_from_pairs(&[
                                                ("type", JsonValue::string("exists")),
                                                (
                                                    "path",
                                                    JsonValue::string("__exec__.payload.is_vip"),
                                                ),
                                            ]),
                                        ),
                                        // VIP 客户：设置折扣 + 触发通知
                                        (
                                            "on_true",
                                            JsonValue::array(vec![
                                                JsonValue::object_from_pairs(&[
                                                    ("type", JsonValue::string("set")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            ("attr", JsonValue::string("discount")),
                                                            ("operation", JsonValue::string("set")),
                                                            ("value", JsonValue::Integer(10)),
                                                        ]),
                                                    ),
                                                ]),
                                                JsonValue::object_from_pairs(&[
                                                    ("type", JsonValue::string("io_request")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            (
                                                                "io_type",
                                                                JsonValue::string("notify_vip"),
                                                            ),
                                                            (
                                                                "prompt",
                                                                JsonValue::string(
                                                                    "VIP 客户享受 9 折优惠",
                                                                ),
                                                            ),
                                                        ]),
                                                    ),
                                                ]),
                                            ]),
                                        ),
                                        // 非 VIP 客户：无折扣
                                        (
                                            "on_false",
                                            JsonValue::array(vec![JsonValue::object_from_pairs(
                                                &[
                                                    ("type", JsonValue::string("set")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            ("attr", JsonValue::string("discount")),
                                                            ("operation", JsonValue::string("set")),
                                                            ("value", JsonValue::Integer(0)),
                                                        ]),
                                                    ),
                                                ],
                                            )]),
                                        ),
                                    ]),
                                ),
                            ]),
                            // 分支 2: 检查库存是否充足
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("branch")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        (
                                            "domain",
                                            JsonValue::object_from_pairs(&[
                                                ("type", JsonValue::string("exists")),
                                                (
                                                    "path",
                                                    JsonValue::string(
                                                        "__exec__.payload.inventory_sufficient",
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        // 库存充足：扣减库存 + 确认通知
                                        (
                                            "on_true",
                                            JsonValue::array(vec![
                                                JsonValue::object_from_pairs(&[
                                                    ("type", JsonValue::string("set")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            (
                                                                "attr",
                                                                JsonValue::string("inventory"),
                                                            ),
                                                            ("operation", JsonValue::string("sub")),
                                                            (
                                                                "value",
                                                                JsonValue::string(
                                                                    "__exec__.payload.quantity",
                                                                ),
                                                            ),
                                                        ]),
                                                    ),
                                                ]),
                                                JsonValue::object_from_pairs(&[
                                                    ("type", JsonValue::string("io_request")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            (
                                                                "io_type",
                                                                JsonValue::string("confirm_order"),
                                                            ),
                                                            (
                                                                "prompt",
                                                                JsonValue::string(
                                                                    "订单已确认，库存已扣减",
                                                                ),
                                                            ),
                                                        ]),
                                                    ),
                                                ]),
                                            ]),
                                        ),
                                        // 库存不足：预警通知
                                        (
                                            "on_false",
                                            JsonValue::array(vec![JsonValue::object_from_pairs(
                                                &[
                                                    ("type", JsonValue::string("io_request")),
                                                    (
                                                        "params",
                                                        JsonValue::object_from_pairs(&[
                                                            (
                                                                "io_type",
                                                                JsonValue::string("stock_warning"),
                                                            ),
                                                            (
                                                                "prompt",
                                                                JsonValue::string(
                                                                    "库存不足，请及时补货",
                                                                ),
                                                            ),
                                                        ]),
                                                    ),
                                                ],
                                            )]),
                                        ),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]),
    ]
}

/// 测试场景 1: VIP 客户 + 库存充足
#[test]
fn test_vip_customer_with_sufficient_inventory() {
    println!("\n=== 测试场景 1: VIP 客户 + 库存充足 ===");

    let core_eval = order_processing_core_eval();

    // 构造 payload
    let payload = JsonValue::object_from_pairs(&[
        ("order_id", JsonValue::string("ORD-001")),
        ("quantity", JsonValue::Integer(5)),
        ("inventory", JsonValue::Integer(100)),
        ("is_vip", JsonValue::Bool(true)),
        ("inventory_sufficient", JsonValue::Bool(true)),
    ]);

    // 构造 instruction
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("process_order")),
        ("params", JsonValue::object_from_pairs(&[])),
    ]);

    println!("初始 payload: {:?}", payload);
    println!("指令: {:?}", instruction);

    // 第一轮执行：应该触发 VIP 通知
    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();

    match result {
        TransitionResult::IoRequired { io_type, params } => {
            println!("✓ 触发 I/O: {}", io_type);
            println!("  参数: {:?}", params);
            assert_eq!(io_type, "notify_vip");

            // 模拟 I/O 完成，注入结果
            let mut updated_payload = payload.clone();
            if let JsonValue::Object(ref mut map) = updated_payload {
                map.insert(
                    "__io_result__".to_string(),
                    JsonValue::string("VIP 通知已发送"),
                );
            }

            // 第二轮执行：应该设置 discount 并继续
            let result2 =
                execute_transition(&core_eval, &instruction, &updated_payload, &[]).unwrap();

            match result2 {
                TransitionResult::State { new_payload, .. } => {
                    println!("✓ 状态更新: {:?}", new_payload);
                    assert_eq!(
                        new_payload.get("discount").and_then(|v| v.as_i64()),
                        Some(10)
                    );
                }
                TransitionResult::IoRequired { io_type, .. } => {
                    println!("✓ 继续触发 I/O: {}", io_type);
                }
            }
        }
        TransitionResult::State { new_payload, .. } => {
            println!("状态更新: {:?}", new_payload);
            // 如果没有触发 I/O，检查 discount 是否设置
            assert_eq!(
                new_payload.get("discount").and_then(|v| v.as_i64()),
                Some(10)
            );
        }
    }
}

/// 测试场景 2: 普通客户 + 库存不足
#[test]
fn test_normal_customer_with_insufficient_inventory() {
    println!("\n=== 测试场景 2: 普通客户 + 库存不足 ===");

    let core_eval = order_processing_core_eval();

    // 构造 payload（无 is_vip 字段，无 inventory_sufficient 字段）
    let payload = JsonValue::object_from_pairs(&[
        ("order_id", JsonValue::string("ORD-002")),
        ("quantity", JsonValue::Integer(10)),
        ("inventory", JsonValue::Integer(5)),
    ]);

    // 构造 instruction
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("process_order")),
        ("params", JsonValue::object_from_pairs(&[])),
    ]);

    println!("初始 payload: {:?}", payload);
    println!("指令: {:?}", instruction);

    // 执行
    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();

    match result {
        TransitionResult::IoRequired { io_type, params } => {
            println!("✓ 触发 I/O: {}", io_type);
            println!("  参数: {:?}", params);
            // 应该触发库存预警（因为 inventory_sufficient 不存在）
            assert_eq!(io_type, "stock_warning");
        }
        TransitionResult::State { new_payload, .. } => {
            println!("状态更新: {:?}", new_payload);
            // 非 VIP，discount 应该为 0
            assert_eq!(
                new_payload.get("discount").and_then(|v| v.as_i64()),
                Some(0)
            );
        }
    }
}

/// 测试场景 3: 非 process_order 指令（不匹配规则）
#[test]
fn test_non_matching_instruction() {
    println!("\n=== 测试场景 3: 非匹配指令 ===");

    let core_eval = order_processing_core_eval();

    let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]);

    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("unknown_instruction")),
        ("params", JsonValue::object_from_pairs(&[])),
    ]);

    println!("初始 payload: {:?}", payload);
    println!("指令: {:?}", instruction);

    // 执行：应该不匹配任何规则，状态不变
    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();

    match result {
        TransitionResult::State { new_payload, .. } => {
            println!("✓ 状态保持不变: {:?}", new_payload);
            assert_eq!(new_payload.get("x").and_then(|v| v.as_i64()), Some(42));
        }
        TransitionResult::IoRequired { .. } => {
            panic!("不应该触发 I/O");
        }
    }
}
