# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 EvoRule Project
# This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
# 仿真确定性对拍编排：native 参照 vs QEMU mps2-an386 固件链尾哈希逐字节一致
#
# 步骤：
#   1. host 构建 + 运行 native 参照（examples/native_parity_tail）→ 基准 CHAIN_TAIL
#   2. thumbv7em-none-eabihf 构建 QEMU 固件（examples/qemu_parity，--features qemu）
#   3. qemu-system-arm -M mps2-an386 跑固件 → QEMU CHAIN_TAIL
#   4. 两哈希逐字节比对，PASS/FAIL 退出码
#
# 用法：powershell -File scripts\qemu_parity.ps1
# 依赖：rustup 目标 thumbv7em-none-eabihf、qemu-system-arm 在 PATH
# （本机脚本先行；CI 化留嵌入式实机批）
# PS 5.1：native 工具（cargo/qemu）的 stderr 告警不构成终止错误，
# 成败一律以 $LASTEXITCODE 显式判定
$ErrorActionPreference = "Continue"
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$Target = "thumbv7em-none-eabihf"
$Elf = "target\$Target\release\examples\qemu_parity"

Write-Host "=== [1/3] native 参照（host）===" -ForegroundColor Cyan
cargo build --release -p evorule-mcu --example native_parity_tail
if ($LASTEXITCODE -ne 0) { throw "native build failed" }
$NativeOut = & cargo run -q -p evorule-mcu --example native_parity_tail 2>$null
if ($LASTEXITCODE -ne 0) { throw "native run failed" }
$NativeTail = ($NativeOut | Select-String -Pattern "^CHAIN_TAIL=(?<h>[0-9a-f]{64})$").Matches[0].Groups["h"].Value
Write-Host "native  CHAIN_TAIL = $NativeTail"

Write-Host "=== [2/3] QEMU 固件构建（$Target）===" -ForegroundColor Cyan
cargo build --release --target $Target -p evorule-mcu --features qemu --example qemu_parity
if ($LASTEXITCODE -ne 0) { throw "firmware build failed" }

Write-Host "=== [3/3] QEMU mps2-an386 仿真 ===" -ForegroundColor Cyan
$QemuOut = & qemu-system-arm -machine mps2-an386 -nographic -semihosting-config enable=on,target=native -kernel $Elf 2>&1
$QemuExit = $LASTEXITCODE
$QemuOut | ForEach-Object { Write-Host "qemu | $_" }
if ($QemuExit -ne 0) { throw "qemu run failed (exit $QemuExit)" }
$QemuTail = ($QemuOut | Select-String -Pattern "CHAIN_TAIL=(?<h>[0-9a-f]{64})").Matches[0].Groups["h"].Value
Write-Host "qemu    CHAIN_TAIL = $QemuTail"

Write-Host ""
if ($NativeTail -eq $QemuTail) {
    Write-Host "PARITY_OK: 链尾哈希逐字节一致" -ForegroundColor Green
    Write-Host "  native = $NativeTail"
    Write-Host "  qemu   = $QemuTail"
    exit 0
} else {
    Write-Host "PARITY_MISMATCH: 链尾哈希不一致" -ForegroundColor Red
    Write-Host "  native = $NativeTail"
    Write-Host "  qemu   = $QemuTail"
    exit 1
}
