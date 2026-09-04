# SPDX-License-Identifier: AGPL-3.0-or-later
#!/usr/bin/env pwsh
# validate-cargolock.ps1
# VERSION_STRATEGY.md 8
# Check: binary projects commit Cargo.lock, lib projects do not
# Exit code: 0 = pass, 1 = fail

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot

# Project -> has binary ([[bin]] section), and the workspace root that contains its Cargo.lock
# evorule-reactor has cdylib but no [[bin]], treat as lib
# evorule-governance 现为纯 lib crate(无 [[bin]])
# 各仓独立发布:仅校验本仓 crate,不查兄弟仓
# Cargo.lock for workspace crates lives at the workspace root, not the crate dir
$projects = [ordered]@{}
$projects['evorule-tcb']        = @{ Path = "$evoruleRoot\evorule-tcb";        HasBinary = $false; LockRoot = $evoruleRoot }
$projects['evorule-reactor']    = @{ Path = "$evoruleRoot\evorule-reactor";    HasBinary = $false; LockRoot = $evoruleRoot }
$projects['evorule-governance'] = @{ Path = "$evoruleRoot\evorule-governance"; HasBinary = $false; LockRoot = $evoruleRoot }

$failed = $false
Write-Host "`n=== Cargo.lock Policy Validation (8) ===" -ForegroundColor Cyan

foreach ($name in $projects.Keys) {
    $p = $projects[$name]
    $lockPath = Join-Path $p.LockRoot "Cargo.lock"
    $giPath = Join-Path $p.LockRoot ".gitignore"

    $lockExists = Test-Path $lockPath
    $giIgnoresLock = $false

    if (Test-Path $giPath) {
        $giContent = Get-Content $giPath -Raw
        $giIgnoresLock = $giContent -match '(?m)^\s*Cargo\.lock\s*$'
    }

    if ($p.HasBinary) {
        # binary: must commit
        if ($lockExists) {
            Write-Host "[OK]   $name (binary) : Cargo.lock exists at $($p.LockRoot)" -ForegroundColor Green
        } else {
            Write-Host "[INFO] $name (binary) : Cargo.lock not yet generated (run 'cargo build')" -ForegroundColor Cyan
        }
        if ($giIgnoresLock) {
            Write-Host "[FAIL] $name (binary) : .gitignore excludes Cargo.lock (must commit)" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   $name (binary) : .gitignore does NOT exclude Cargo.lock" -ForegroundColor Green
        }
    } else {
        # lib: optional
        if ($giIgnoresLock) {
            Write-Host "[OK]   $name (lib) : .gitignore excludes Cargo.lock" -ForegroundColor Green
        } else {
            Write-Host "[INFO] $name (lib) : .gitignore does NOT exclude Cargo.lock (optional)" -ForegroundColor Cyan
        }
    }
}

if ($failed) { Write-Host "`n[RESULT] FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
