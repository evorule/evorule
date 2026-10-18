// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 构建脚本：仅对嵌入式 none 目标接入 cortex-m-rt 链接脚本
//!
//! - `-Tlink.x`：cortex-m-rt 启动段（向量表 / `_start` / section 布局），
//!   由 cortex-m-rt 的 build 脚本暴露 link-search
//! - `-Tmemory.x`：mps2-an386 存储布局（QEMU 仿真目标），由本包 memory.x
//!   提供，link-search 指向本包根目录
//!
//! host（std）构建完全不受影响（TARGET 门控），link 参数仅作用于
//! bins/examples（`rustc-link-arg-bins`），库目标零影响。

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    // 覆盖 thumbv*-none-eabi(hf) / riscv*-none-elf 等 no_std 裸机目标
    if target.contains("-none") {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        // link.x 内部 INCLUDE memory.x：经 link-search 命中本包 memory.x
        //（mps2-an386 布局）；显式 -Tmemory.x 会与之重复定义 region
        println!("cargo:rustc-link-search=native={manifest_dir}");
        println!("cargo:rustc-link-arg=-Tlink.x");
    }
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
