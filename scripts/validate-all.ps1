#!/usr/bin/env pwsh
# validate-all.ps1
# One-shot runner for all VERSION_STRATEGY validation scripts
# Usage: pwsh scripts/validate-all.ps1 [-AllowUnreleased]

[CmdletBinding()]
param(
    [switch]$AllowUnreleased
)

$scripts = @(
    @{ Name = 'validate-version';   File = 'validate-version.ps1' }
    @{ Name = 'validate-changelog'; File = 'validate-changelog.ps1' }
    @{ Name = 'validate-license';   File = 'validate-license.ps1' }
    @{ Name = 'validate-cargolock'; File = 'validate-cargolock.ps1' }
    @{ Name = 'validate-release';   File = 'validate-release.ps1' }
)

$failed = @()
foreach ($s in $scripts) {
    Write-Host "`n>>> [$($s.Name)] <<<" -ForegroundColor Magenta
    $scriptPath = "$PSScriptRoot\$($s.File)"
    if ($s.Name -eq 'validate-changelog' -and $AllowUnreleased) {
        & $scriptPath -AllowUnreleased
    } else {
        & $scriptPath
    }
    if ($LASTEXITCODE -ne 0) {
        $failed += $s.Name
    }
}

Write-Host "`n=========== SUMMARY ===========" -ForegroundColor Cyan
if ($failed.Count -gt 0) {
    Write-Host "FAILED: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host "ALL 5 VALIDATION SCRIPTS PASSED" -ForegroundColor Green
exit 0
