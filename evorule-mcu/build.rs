// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Build script: wires in the cortex-m-rt linker script only for embedded
//! none targets
//!
//! - `-Tlink.x`: cortex-m-rt startup sections (vector table / `_start` /
//!   section layout); the link-search path is exposed by cortex-m-rt's
//!   build script
//! - `-Tmemory.x`: the mps2-an386 memory layout (QEMU simulation target),
//!   provided by this package's memory.x; link-search points at this
//!   package's root directory
//!
//! host (std) builds are completely unaffected (TARGET gate); the link
//! arguments apply only to bins/examples (`rustc-link-arg-bins`); library
//! targets are untouched.

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    // Covers no_std bare-metal targets such as thumbv*-none-eabi(hf) / riscv*-none-elf
    if target.contains("-none") {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        // link.x INCLUDEs memory.x internally: the link-search path hits this
        // package's memory.x (the mps2-an386 layout); an explicit -Tmemory.x
        // would redefine the regions
        println!("cargo:rustc-link-search=native={manifest_dir}");
        println!("cargo:rustc-link-arg=-Tlink.x");
    }
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
