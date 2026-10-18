// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! native 参照：I/O 暂停-恢复场景 host 编译的链尾哈希（仿真对拍基准值）
//!
//! # 判据链（I/O 恢复跨目标对拍）
//! 1. 对拍单测（`exec::tests`）已证：I/O 暂停-恢复全链事实序列
//!    （Command / IoRequest / IoResponse / StateTransition / TransitionTrace /
//!    Stable）与 reactor v0.3.1 handle_fact IoResponse 分支逐行同构
//! 2. 本例执行同场景（io_resume fixture 规则包 + call_external 指令 +
//!    固定应答）打印 host 内核链尾（基准值）
//! 3. `examples/qemu_io_resume`（mps2-an386 固件）输出与之逐字节一致
//!    → I/O 恢复协议跨目标确定性成立
//!
//! # 场景
//! `tests/fixtures/io_resume_rules.evopack`（双路径 I/O 循环规则）+
//! call_external 指令（messages 透传）；run 触发 IoRequest 暂停后，
//! 以固定应答（`canned_llm_result` 同值：`{"role":"assistant",
//! "content":"no tools needed"}`）resume；期望终态 `llm_response.content`
//! 消费到位且 `__io_results__` 容器已清除。
//!
//! 运行：`cargo run -p evorule-mcu --example native_io_resume_tail`
//! 输出：`IO_CHAIN_TAIL=<64 hex>`（stdout），进度走 stderr。

use std::process::ExitCode;

use evorule_mcu::{McuError, McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;

/// I/O 对拍 fixture（host 编译器 `compile-evopack` 产物，与 QEMU 固件同一文件）
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/io_resume_rules.evopack");

fn call_external_instruction(content: &str) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "messages",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("role", JsonValue::string("user")),
                        ("content", JsonValue::string(content)),
                    ])]),
                ),
                ("tools", JsonValue::array(vec![])),
            ]),
        ),
    ])
}

/// 固定 LLM 应答（与内核 `canned_llm_result` 逐字段同值，对拍确定性）
fn canned_llm_result() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("role", JsonValue::string("assistant")),
        ("content", JsonValue::string("no tools needed")),
    ])
}

fn main() -> ExitCode {
    let pack = match RulePack::from_evopack(EVOPACK) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("E: fixture evopack decode failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut ex = McuExecutor::new(pack);

    // run → IoRequest 入链后暂停
    let request_id = match ex.run(call_external_instruction("hello from io parity")) {
        Err(McuError::IoPaused {
            request_id,
            io_type,
        }) => {
            eprintln!("native paused: request_id={request_id} io_type={io_type}");
            if io_type != "call_external" {
                eprintln!("E: unexpected io_type {io_type}");
                return ExitCode::FAILURE;
            }
            request_id
        }
        Ok(_) => {
            eprintln!("E: expected IoPaused, got Ok");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("E: run failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // 固定应答恢复
    match ex.resume_io(request_id, Ok(canned_llm_result())) {
        Ok(report) => eprintln!(
            "native resumed: steps={} ticks={} version={}",
            report.steps_executed, report.ticks, report.version
        ),
        Err(e) => {
            eprintln!("E: resume failed: {e}");
            return ExitCode::FAILURE;
        }
    }

    // 场景终态自检：llm_response 消费到位，容器清除不残留
    match ex
        .payload()
        .get("llm_response")
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
    {
        Some("no tools needed") => {}
        other => {
            eprintln!("E: llm_response.content expected \"no tools needed\", got {other:?}");
            return ExitCode::FAILURE;
        }
    }
    if ex.payload().get("__io_results__").is_some() {
        eprintln!("E: __io_results__ container not cleared");
        return ExitCode::FAILURE;
    }

    println!("IO_CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    ExitCode::SUCCESS
}
