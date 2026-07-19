# add-spdx-safe.ps1
# SAFE SPDX header adder using byte-level operations.
# This replaces the buggy fix-missing-spdx.ps1 which used Get-Content -Raw
# and corrupted UTF-8 files (without BOM) on Chinese Windows (PowerShell 5.1
# decoded them as GBK, garbling Chinese text and substituting some bytes with 0x3F).
#
# Strategy: ReadAllBytes -> detect BOM -> check existing SPDX -> insert header
# as bytes -> WriteAllBytes. Never decodes the file as text, so encoding is
# preserved exactly.
#
# Usage:
#   pwsh scripts/add-spdx-safe.ps1 -RootDir D:\evorule\tier0-tcb\src
#   pwsh scripts/add-spdx-safe.ps1 -Paths file1.rs,file2.rs
#   pwsh scripts/add-spdx-safe.ps1 -RootDir D:\evo-agent\src -Exclude "*ffi.rs"
#
# Exit code: 0 = all OK, 1 = some errors

[CmdletBinding()]
param(
    [string[]]$Paths,
    [string]$RootDir,
    [string[]]$Exclude = @()
)

$ErrorActionPreference = "Stop"

# SPDX header as bytes (UTF-8). 4 lines: 3 header + 1 blank.
$SPDX_BYTES = [System.Text.Encoding]::UTF8.GetBytes(
    "// SPDX-License-Identifier: AGPL-3.0-or-later`n" +
    "// Copyright (C) 2026 EvoRule Project`n" +
    "// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.`n`n"
)

# Build file list
$files = @()
if ($Paths) {
    foreach ($p in $Paths) {
        if (Test-Path $p) {
            $files += (Resolve-Path $p).Path
        } else {
            Write-Host "MISSING: $p" -ForegroundColor Red
        }
    }
}
if ($RootDir) {
    if (-not (Test-Path $RootDir)) {
        Write-Host "RootDir not found: $RootDir" -ForegroundColor Red
        exit 1
    }
    $rsFiles = Get-ChildItem -Path $RootDir -Filter "*.rs" -Recurse -ErrorAction SilentlyContinue
    $files += $rsFiles.FullName
}

if ($files.Count -eq 0) {
    Write-Host "No files to process. Use -Paths or -RootDir." -ForegroundColor Yellow
    exit 0
}

$added = 0
$skipped = 0
$errors = 0

foreach ($path in $files) {
    # Apply exclude patterns
    $excluded = $false
    foreach ($pattern in $Exclude) {
        if ($path -like $pattern) {
            $excluded = $true
            break
        }
    }
    if ($excluded) {
        Write-Host "SKIP (excluded): $path" -ForegroundColor Yellow
        $skipped++
        continue
    }

    if (-not (Test-Path $path)) {
        Write-Host "MISSING: $path" -ForegroundColor Red
        $errors++
        continue
    }

    $bytes = [System.IO.File]::ReadAllBytes($path)

    # Detect UTF-8 BOM (EF BB BF)
    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    $contentStart = if ($hasBom) { 3 } else { 0 }

    # Check if SPDX already present (look in first 200 bytes after BOM)
    $checkEnd = [Math]::Min($contentStart + 200, $bytes.Length)
    $headerSlice = [System.Text.Encoding]::UTF8.GetString($bytes[$contentStart..($checkEnd - 1)])
    if ($headerSlice -match 'SPDX-License-Identifier: AGPL-3.0') {
        Write-Host "SKIP (has SPDX): $path" -ForegroundColor Yellow
        $skipped++
        continue
    }

    # Insert SPDX bytes after BOM
    $newLength = $bytes.Length + $SPDX_BYTES.Length
    $newBytes = New-Object byte[] $newLength

    if ($hasBom) {
        # [BOM(3)] + [SPDX] + [content after BOM]
        [Array]::Copy($bytes, 0, $newBytes, 0, 3)
        [Array]::Copy($SPDX_BYTES, 0, $newBytes, 3, $SPDX_BYTES.Length)
        [Array]::Copy($bytes, 3, $newBytes, 3 + $SPDX_BYTES.Length, $bytes.Length - 3)
    } else {
        # [SPDX] + [content]
        [Array]::Copy($SPDX_BYTES, 0, $newBytes, 0, $SPDX_BYTES.Length)
        [Array]::Copy($bytes, 0, $newBytes, $SPDX_BYTES.Length, $bytes.Length)
    }

    try {
        [System.IO.File]::WriteAllBytes($path, $newBytes)
        Write-Host "ADDED: $path" -ForegroundColor Green
        $added++
    } catch {
        Write-Host "ERROR: $path - $_" -ForegroundColor Red
        $errors++
    }
}

Write-Host "`n========== SUMMARY ==========" -ForegroundColor Cyan
Write-Host "Added:   $added" -ForegroundColor Green
Write-Host "Skipped: $skipped" -ForegroundColor Yellow
Write-Host "Errors:  $errors" -ForegroundColor Red

if ($errors -gt 0) { exit 1 }
exit 0
