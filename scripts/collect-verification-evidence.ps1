# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 EvoRule Project
# This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
# =============================================================================
# collect-verification-evidence.ps1
#
# Collects formal-verification evidence (PASS logs + metadata) into per-crate
# verification/evidence/ directories. Part of Phase 0 (evidence archiving) of
# the formal verification improvement plan.
#
# Evidence metadata per run:
#   - commit SHA (git rev-parse HEAD)
#   - toolchain version (rustc/cargo --version)
#   - timestamp
#   - platform (OS)
#   - test command + PROPTEST_CASES
#   - PASS/FAIL log
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/collect-verification-evidence.ps1
#
# NOTE: Keep this script ASCII-only (no non-ASCII bytes) so it parses cleanly
# under Windows PowerShell 5.1 without BOM/encoding issues.
# =============================================================================

$RepoRoot = Split-Path -Parent $PSScriptRoot

function Get-Metadata {
    param([string]$LogPath)
    $commit = ""
    try { $commit = git rev-parse HEAD } catch { $commit = "unknown" }
    $rustc = "unknown"
    try { $rustc = (rustc --version 2>$null) } catch {}
    $cargo = "unknown"
    try { $cargo = (cargo --version 2>$null) } catch {}
    $time = Get-Date -Format "yyyy-MM-dd HH:mm:ss zzz"
    $os = [System.Environment]::OSVersion.VersionString

    return @"
# Evidence metadata
- commit SHA: $commit
- rustc: $rustc
- cargo: $cargo
- timestamp: $time
- platform: $os
- log: $LogPath
"@
}

function Invoke-Capture {
    param(
        [string]$Command,
        [string]$Package,
        [string]$TestTarget,
        [string]$EvidenceDir,
        [string]$Label
    )
    New-Item -ItemType Directory -Force -Path $EvidenceDir | Out-Null
    $ts = Get-Date -Format "yyyyMMdd_HHmmss"
    $commit = "unknown"
    try { $commit = git rev-parse --short HEAD } catch {}
    $logFile = Join-Path $EvidenceDir "$Label`_PASS_$commit`_$ts.log"
    $stdout = Join-Path $EvidenceDir "$Label`_PASS_$commit`_$ts.stdout.txt"

    Write-Host "== Running: $Command"
    # Redirect output inside cmd.exe so PowerShell 5.1 never sees native
    # stderr (cargo warnings) as errors; exit code captured via $LASTEXITCODE.
    # NOTE: do NOT wrap $Command in extra quotes - cmd would then treat the
    # quoted string as a command name instead of command + args.
    $full = "$Command > `"$stdout`" 2>&1"
    cmd /c $full | Out-Null
    $exit = $LASTEXITCODE

    $status = if ($exit -eq 0) { "PASS" } else { "FAIL" }
    $meta = Get-Metadata -LogPath (Split-Path -Leaf $stdout)
    $tail = if (Test-Path $stdout) { Get-Content $stdout | Select-Object -Last 40 } else { @("(no output)") }
    $body = @"
Verification evidence: $Label
Status: $status
Exit code: $exit
$meta
--- captured output (see stdout file) ---
$tail
"@
    $body | Out-File -FilePath $logFile -Encoding utf8
    Write-Host "== [$status] Evidence archived: $logFile"
    if ($exit -ne 0) {
        Write-Host "== ERROR: $Label failed (exit $exit). Full stdout: $stdout"
        exit $exit
    }
}

# ---- P0-12: reactor differential test ----
Invoke-Capture `
    -Command "cargo test --package evorule-reactor --test differential_test" `
    -Package "evorule-reactor" `
    -TestTarget "differential_test" `
    -EvidenceDir (Join-Path $RepoRoot "evorule-reactor\verification\evidence\differential") `
    -Label "P0-12"

# ---- P0-9 / P0-10: governance differential test ----
Invoke-Capture `
    -Command "cargo test --package evorule-governance --test differential_test" `
    -Package "evorule-governance" `
    -TestTarget "differential_test" `
    -EvidenceDir (Join-Path $RepoRoot "evorule-governance\verification\evidence\differential") `
    -Label "P0-9-P0-10"

Write-Host "== All differential evidence collected."
