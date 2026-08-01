#!/usr/bin/env pwsh
# validate-license.ps1
# Check: LICENSE file + AGPL/CC0 identifier + .rs SPDX header + SDK license field
# Exit code: 0 = pass, 1 = fail

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot

$failed = $false
Write-Host "`n=== License Validation ===" -ForegroundColor Cyan

# 1. LICENSE file + AGPL content(各仓独立发布:仅校验本仓)
$licenseChecks = @(
    @{ Name = 'evorule';     Path = "$evoruleRoot\LICENSE" }
)
foreach ($p in $licenseChecks) {
    if (-not (Test-Path $p.Path)) {
        Write-Host "[FAIL] $($p.Name) : LICENSE not found at $($p.Path)" -ForegroundColor Red
        $failed = $true
        continue
    }
    $content = Get-Content $p.Path -Raw
    if ($content -match 'GNU Affero General Public License|AGPL-3.0') {
        Write-Host "[OK]   $($p.Name) : LICENSE contains AGPL" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $($p.Name) : LICENSE does not contain AGPL" -ForegroundColor Red
        $failed = $true
    }
}

# 2. SDK TypeScript license field
$tsPkg = "$evoruleRoot\sdk\typescript\package.json"
if (Test-Path $tsPkg) {
    $pkg = Get-Content $tsPkg -Raw | ConvertFrom-Json
    if ($pkg.license -match 'AGPL-3.0') {
        Write-Host "[OK]   sdk-typescript : license = AGPL-3.0" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] sdk-typescript : license is '$($pkg.license)', expected AGPL-3.0" -ForegroundColor Red
        $failed = $true
    }
}

# 3. SDK Python license field (pyproject.toml format: license = { text = "..." })
$pyToml = "$evoruleRoot\sdk\python\pyproject.toml"
if (Test-Path $pyToml) {
    $toml = Get-Content $pyToml -Raw
    if ($toml -match 'license\s*=\s*\{[^}]*text\s*=\s*"AGPL-3.0"') {
        Write-Host "[OK]   sdk-python : license = AGPL-3.0" -ForegroundColor Green
    } else {
        $cur = if ($toml -match 'license\s*=\s*\{[^}]*text\s*=\s*"([^"]+)"') { $Matches[1] } else { '(none)' }
        Write-Host "[FAIL] sdk-python : license is '$cur', expected AGPL-3.0" -ForegroundColor Red
        $failed = $true
    }
}

# 4. .rs files SPDX header(各仓独立发布:仅校验本仓 src)
$rsDirs = @(
    "$evoruleRoot\evorule-tcb\src",
    "$evoruleRoot\evorule-reactor\src",
    "$evoruleRoot\evorule-governance\src"
)
$totalRs = 0
$withSpdx = 0
$missingFiles = @()
foreach ($dir in $rsDirs) {
    if (-not (Test-Path $dir)) { continue }
    $rsFiles = Get-ChildItem -Path $dir -Filter "*.rs" -Recurse
    foreach ($f in $rsFiles) {
        $totalRs++
        $head = Get-Content $f.FullName -TotalCount 12 -ErrorAction SilentlyContinue
        if ($head -join "`n" -match 'SPDX-License-Identifier:\s*AGPL-3.0') {
            $withSpdx++
        } else {
            $missingFiles += $f.FullName.Replace($evoruleRoot, '.')
        }
    }
}
if ($totalRs -eq 0) {
    Write-Host "[WARN] No .rs files found" -ForegroundColor Yellow
} elseif ($withSpdx -eq $totalRs) {
    Write-Host "[OK]   All $totalRs .rs files have SPDX header" -ForegroundColor Green
} else {
    $missing = $totalRs - $withSpdx
    Write-Host "[FAIL] $missing / $totalRs .rs files missing SPDX header" -ForegroundColor Red
    foreach ($f in $missingFiles | Select-Object -First 5) {
        Write-Host "       - $f" -ForegroundColor DarkRed
    }
    if ($missingFiles.Count -gt 5) {
        Write-Host "       ... and $($missingFiles.Count - 5) more" -ForegroundColor DarkRed
    }
    $failed = $true
}

# 5. core_eval.json CC0-1.0 metadata
$coreEval = "$evoruleRoot\evorule-tcb\core_eval.json"
if (Test-Path $coreEval) {
    $content = Get-Content $coreEval -Raw
    if ($content -match 'CC0-1\.0') {
        Write-Host "[OK]   core_eval.json : CC0-1.0 metadata present" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] core_eval.json : CC0-1.0 metadata missing" -ForegroundColor Red
        $failed = $true
    }
}

if ($failed) { Write-Host "`n[RESULT] FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
