# scripts/add_spdx_headers.ps1
# DEPRECATED wrapper: forwards to add-spdx-safe.ps1 (byte-level, BOM-safe).
#
# The original implementation used Get-Content -Raw -Encoding utf8, which on
# PowerShell 5.1 can corrupt UTF-8 files without BOM (Chinese Windows decodes
# as GBK, garbling Chinese). The new add-spdx-safe.ps1 uses ReadAllBytes and
# operates on bytes directly, so it is encoding-safe.
#
# This file is kept for backward compatibility with existing invocations
# and CI pipelines. New code should call add-spdx-safe.ps1 directly.
#
# Excludes: evorule-reactor/src/ffi.rs (allows unsafe_code, known)

$ErrorActionPreference = "Stop"

$safeScript = Join-Path $PSScriptRoot "add-spdx-safe.ps1"
if (-not (Test-Path $safeScript)) {
    Write-Host "ERROR: $safeScript not found" -ForegroundColor Red
    exit 1
}

Write-Host "[add_spdx_headers.ps1] Delegating to add-spdx-safe.ps1" -ForegroundColor Cyan
Write-Host ""

# Call safe script for each tier, with the original exclusion
& powershell -NoProfile -File $safeScript -RootDir "D:\evorule\evorule-tcb\src"
$rc1 = $LASTEXITCODE

& powershell -NoProfile -File $safeScript -RootDir "D:\evorule\evorule-reactor\src" -Exclude "*ffi.rs"
$rc2 = $LASTEXITCODE

& powershell -NoProfile -File $safeScript -RootDir "D:\evorule\evorule-governance\src"
$rc3 = $LASTEXITCODE

# Aggregate exit code
if ($rc1 -ne 0 -or $rc2 -ne 0 -or $rc3 -ne 0) {
    exit 1
}
exit 0
